use super::*;
use crate::test_support::ScratchDir;
use axum::http::Uri;
use std::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// How long a test waits for the bytes it expects before it fails.
const READ_TIMEOUT: Duration = Duration::from_secs(5);

/// An HTTP/1 upgrade request for `path`.
fn upgrade_request(path: &str) -> String {
    format!(
        "GET {path} HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n"
    )
}

/// Read from `socket` until the buffer holds `needle`.
async fn read_until(socket: &mut TcpStream, needle: &[u8]) -> String {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 512];
    loop {
        if buffer.windows(needle.len()).any(|window| window == needle) {
            return String::from_utf8_lossy(&buffer).into_owned();
        }
        let read = tokio::time::timeout(READ_TIMEOUT, socket.read(&mut chunk))
            .await
            .expect("the tunnel must answer in time")
            .expect("read the tunnel");
        assert_ne!(
            read,
            0,
            "the tunnel closed early: {:?}",
            String::from_utf8_lossy(&buffer)
        );
        buffer.extend_from_slice(&chunk[..read]);
    }
}

/// A raw HTTP/1 upgrade server that answers `101` and then echoes every byte.
///
/// The `101` carries the request line it received, so a test can assert the
/// path the proxy sent upstream.
async fn upgrade_upstream() -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind an upgrade port");
    let port = listener.local_addr().expect("upgrade address").port();
    let handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept the upgrade");
        let mut buffer = Vec::new();
        let mut chunk = [0_u8; 256];
        while !buffer.windows(4).any(|window| window == b"\r\n\r\n") {
            let read = socket
                .read(&mut chunk)
                .await
                .expect("read the request head");
            if read == 0 {
                return;
            }
            buffer.extend_from_slice(&chunk[..read]);
        }
        let request_line = String::from_utf8_lossy(&buffer)
            .lines()
            .next()
            .unwrap_or_default()
            .to_string();
        let reply = format!(
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n{request_line}\n"
        );
        socket
            .write_all(reply.as_bytes())
            .await
            .expect("answer the upgrade");
        while let Ok(read) = socket.read(&mut chunk).await {
            if read == 0 || socket.write_all(&chunk[..read]).await.is_err() {
                return;
            }
        }
    });
    (format!("http://127.0.0.1:{port}"), handle)
}

/// A stub upstream that names itself in every response.
async fn stub_upstream(name: &'static str) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind a stub port");
    let port = listener.local_addr().expect("stub address").port();
    let app =
        Router::new().fallback(move |uri: Uri| async move { format!("{name}:{}", uri.path()) });
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://127.0.0.1:{port}"), handle)
}

/// Start the proxy on an ephemeral port with the given topology.
async fn start_proxy(topology: DevTopology) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind the proxy port");
    let port = listener.local_addr().expect("proxy address").port();
    tokio::spawn(async move {
        let _ = serve_proxy(listener, topology).await;
    });
    port
}

/// An origin nothing listens on.
fn dead_origin() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind a throwaway port");
    let port = listener.local_addr().expect("throwaway address").port();
    drop(listener);
    format!("http://127.0.0.1:{port}")
}

fn frontend_project(name: &str, config_file: &str) -> ScratchDir {
    let dir = ScratchDir::new(name);
    fs::write(dir.join(config_file), "export default {}\n").expect("write the frontend config");
    dir
}

#[test]
fn detects_each_supported_frontend() {
    let cases = [
        ("vite.config.ts", "vite", 5173),
        ("rsbuild.config.mjs", "rsbuild", 3000),
        ("next.config.js", "next", 3000),
        ("webpack.config.cjs", "webpack", 8080),
    ];
    for (config_file, framework, default_port) in cases {
        let dir = frontend_project(&format!("dev-detect-{framework}"), config_file);
        let detected = detect_frontend(&dir).expect("the frontend must be detected");
        assert_eq!(detected.framework, framework);
        assert_eq!(detected.default_port, default_port);
    }
}

#[test]
fn a_directory_without_a_frontend_config_detects_nothing() {
    let dir = ScratchDir::new("dev-detect-none");
    fs::write(dir.join("package.json"), "{}").expect("write a package.json");
    assert!(detect_frontend(&dir).is_none());
}

#[tokio::test]
async fn the_proxy_sends_each_path_to_its_owner() {
    let (backend, backend_task) = stub_upstream("backend").await;
    let (frontend, frontend_task) = stub_upstream("frontend").await;
    let port = start_proxy(DevTopology {
        backend,
        frontend: Some(frontend),
        routes: vec!["/ping".to_string()],
    })
    .await;

    let client = reqwest::Client::new();
    let get = |path: &str| {
        let url = format!("http://127.0.0.1:{port}{path}");
        let client = client.clone();
        async move {
            client
                .get(url)
                .send()
                .await
                .expect("the proxy must answer")
                .text()
                .await
                .expect("a text body")
        }
    };

    assert_eq!(get("/ping").await, "backend:/ping");
    // The prefix reaches the backend as a path it serves.
    assert_eq!(get("/api/users").await, "backend:/users");
    assert_eq!(get("/").await, "frontend:/");
    assert_eq!(get("/assets/app.js").await, "frontend:/assets/app.js");

    backend_task.abort();
    frontend_task.abort();
}

#[tokio::test]
async fn an_upstream_that_does_not_answer_reports_502() {
    let dead = dead_origin();
    let port = start_proxy(DevTopology {
        backend: dead.clone(),
        frontend: Some(dead),
        routes: vec![],
    })
    .await;

    let response = reqwest::Client::new()
        .get(format!("http://127.0.0.1:{port}/"))
        .send()
        .await
        .expect("the proxy must answer");
    assert_eq!(response.status().as_u16(), 502);
    let body = response.text().await.expect("a text body");
    assert!(body.contains("did not answer"), "{body}");
}

#[tokio::test]
async fn the_proxy_rewrites_the_api_prefix_and_forwards_the_query_and_body() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind the backend port");
    let backend_port = listener.local_addr().expect("backend address").port();
    let app = Router::new().fallback(|request: Request| async move {
        let path = request.uri().path().to_string();
        let query = request.uri().query().unwrap_or_default().to_string();
        let body = axum::body::to_bytes(request.into_body(), 1024)
            .await
            .unwrap_or_default();
        format!("{path}|{query}|{}", String::from_utf8_lossy(&body))
    });
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let port = start_proxy(DevTopology {
        backend: format!("http://127.0.0.1:{backend_port}"),
        frontend: None,
        routes: vec![],
    })
    .await;

    let body = reqwest::Client::new()
        .post(format!("http://127.0.0.1:{port}/api/echo?trace=1"))
        .body("{\"hello\":\"world\"}")
        .send()
        .await
        .expect("the proxy must answer")
        .text()
        .await
        .expect("a text body");
    // The backend has no `/api` mount, so the proxy strips the prefix.
    assert_eq!(body, "/echo|trace=1|{\"hello\":\"world\"}");

    handle.abort();
}

#[tokio::test]
async fn the_proxy_tunnels_an_upgrade_request_to_the_frontend() {
    let (frontend, frontend_task) = upgrade_upstream().await;
    let port = start_proxy(DevTopology {
        backend: "http://127.0.0.1:1".to_string(),
        frontend: Some(frontend),
        routes: vec![],
    })
    .await;

    let mut socket = TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect to the proxy");
    socket
        .write_all(upgrade_request("/hmr?token=1").as_bytes())
        .await
        .expect("write the upgrade request");

    // The proxy relays the 101, and the tunnel then carries the frontend's
    // echo of the request line it received.
    let reply = read_until(&mut socket, b"GET /hmr?token=1 HTTP/1.1").await;
    assert!(reply.starts_with("HTTP/1.1 101"), "{reply}");
    assert!(reply.contains("upgrade: websocket"), "{reply}");

    // Both directions stay open: a later frame reaches the frontend and its
    // echo comes back.
    socket
        .write_all(b"ping")
        .await
        .expect("write through the tunnel");
    let echo = read_until(&mut socket, b"ping").await;
    assert_eq!(echo.matches("ping").count(), 1, "{echo}");

    frontend_task.abort();
}

#[tokio::test]
async fn the_tunnel_rewrites_the_api_prefix_on_its_way_upstream() {
    // An upgrade request on the API prefix belongs to the backend, and the
    // backend serves the blueprint's paths without the prefix.
    let (backend, backend_task) = upgrade_upstream().await;
    let port = start_proxy(DevTopology {
        backend,
        frontend: None,
        routes: vec![],
    })
    .await;

    let mut socket = TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect to the proxy");
    socket
        .write_all(upgrade_request("/api/hmr").as_bytes())
        .await
        .expect("write the upgrade request");

    let reply = read_until(&mut socket, b"GET /hmr HTTP/1.1").await;
    assert!(reply.starts_with("HTTP/1.1 101"), "{reply}");

    backend_task.abort();
}

#[tokio::test]
async fn a_frontend_that_refuses_the_upgrade_relays_its_answer() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind the refusing frontend");
    let frontend_port = listener.local_addr().expect("frontend address").port();
    let handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept the upgrade");
        let mut buffer = Vec::new();
        let mut chunk = [0_u8; 256];
        while !buffer.windows(4).any(|window| window == b"\r\n\r\n") {
            let read = socket
                .read(&mut chunk)
                .await
                .expect("read the request head");
            if read == 0 {
                return;
            }
            buffer.extend_from_slice(&chunk[..read]);
        }
        let _ = socket
            .write_all(
                b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
            )
            .await;
    });

    let port = start_proxy(DevTopology {
        backend: "http://127.0.0.1:1".to_string(),
        frontend: Some(format!("http://127.0.0.1:{frontend_port}")),
        routes: vec![],
    })
    .await;

    // The request must ask for the upgrade, or it never reaches the tunnel.
    let mut socket = TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect to the proxy");
    socket
        .write_all(upgrade_request("/hmr").as_bytes())
        .await
        .expect("write the upgrade request");

    let reply = read_until(&mut socket, b"\r\n\r\n").await;
    assert!(reply.starts_with("HTTP/1.1 400"), "{reply}");
    // The relayed head carries an accurate length: the client must not wait
    // for a body that never comes.
    assert!(reply.contains("content-length: 0"), "{reply}");

    handle.abort();
}

#[tokio::test]
async fn a_frontend_that_refuses_the_tunnel_connection_reports_502() {
    let port = start_proxy(DevTopology {
        backend: "http://127.0.0.1:1".to_string(),
        frontend: Some(dead_origin()),
        routes: vec![],
    })
    .await;

    let mut socket = TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect to the proxy");
    socket
        .write_all(upgrade_request("/hmr").as_bytes())
        .await
        .expect("write the upgrade request");

    let reply = read_until(&mut socket, b"\r\n\r\n").await;
    assert!(reply.starts_with("HTTP/1.1 502"), "{reply}");
}

#[test]
fn the_frontend_port_may_not_take_the_proxy_port() {
    let dir = frontend_project("dev-port-clash", "vite.config.ts");
    let app = dir.join("app.py");
    fs::write(&app, "from rivet import api\n").expect("write app.py");
    fs::write(
        dir.join("rivet.toml"),
        "[environments]\ndevelopment = { host = \"127.0.0.1\", port = 3000 }\n",
    )
    .expect("write rivet.toml");

    let diagnostics = run_dev(&app, Some(3000), None).expect_err("the clash must be refused");
    assert_eq!(diagnostics[0].error_code, "E3018");
    assert!(diagnostics[0].message.contains("3000"));
}
