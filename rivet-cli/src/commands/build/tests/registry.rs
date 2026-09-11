//! The registry integration tests: a stub registry as the oracle.

use super::*;

/// A stub service registry: it records every request it receives and either
/// answers `200` or never answers at all.
///
/// The answering mode pins the wire format the generated client must speak:
/// the request line, the headers, and the body the client sends.
struct StubRegistry {
    /// The port the registry listens on.
    port: u16,
    /// Every request, head and body, in arrival order.
    requests: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl StubRegistry {
    /// Start a stub that answers every request with `200`.
    fn answering() -> StubRegistry {
        Self::start(true)
    }

    /// Start a stub that accepts a connection and never answers it.
    fn silent() -> StubRegistry {
        Self::start(false)
    }

    fn start(answering: bool) -> StubRegistry {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind the stub registry");
        let port = listener.local_addr().expect("stub registry address").port();
        let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = std::sync::Arc::clone(&requests);

        // A detached thread: the test binary ends the process, so the accept
        // loop needs no stop flag.
        std::thread::spawn(move || {
            // A silent stub holds each connection open, so the client waits
            // on its own deadline instead of reading an end of stream.
            let mut held = Vec::new();
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let request = match read_one_request(&mut stream) {
                    Some(request) => request,
                    None => continue,
                };
                if let Ok(mut recorded) = recorded.lock() {
                    recorded.push(request);
                }
                if answering {
                    let _ = stream.write_all(
                        b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}",
                    );
                } else {
                    held.push(stream);
                }
            }
        });
        StubRegistry { port, requests }
    }

    /// The requests the stub captured so far.
    fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("stub registry records").clone()
    }
}

/// Read one complete HTTP request: its head, then the body its
/// `content-length` names.
fn read_one_request(stream: &mut TcpStream) -> Option<String> {
    let mut raw = Vec::new();
    let mut buffer = [0u8; 1024];
    loop {
        let read = stream.read(&mut buffer).ok()?;
        if read == 0 {
            return None;
        }
        raw.extend_from_slice(&buffer[..read]);
        let Some(end) = raw.windows(4).position(|window| window == b"\r\n\r\n") else {
            continue;
        };
        let head = String::from_utf8_lossy(&raw[..end]).into_owned();
        let length: usize = head
            .lines()
            .filter_map(|line| line.split_once(':'))
            .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
            .and_then(|(_, value)| value.trim().parse().ok())
            .unwrap_or(0);
        if raw.len() >= end + 4 + length {
            return Some(String::from_utf8_lossy(&raw).into_owned());
        }
    }
}

/// Send a signal to a running generated server and wait for it to exit.
fn signal_and_wait(child: &mut Child, signal: &str) {
    let status = Command::new("kill")
        .arg(format!("-{signal}"))
        .arg(child.id().to_string())
        .status()
        .expect("run kill");
    assert!(status.success(), "kill -{signal} must reach the server");

    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if let Ok(Some(_)) = child.try_wait() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("the server did not exit after SIG{signal}");
}

#[test]
fn the_app_registers_with_the_registry_and_leaves_it_on_shutdown() {
    let dir = ScratchDir::new("discovery-register");
    let app = dir.join("app.py");
    fs::write(&app, FIXTURE_APP).expect("write app.py");
    let registry = StubRegistry::answering();
    fs::write(
        dir.join("rivet.toml"),
        format!(
            "[project]\nname = \"orders\"\n\n[discovery]\nbackend = \"consul\"\nurl = \"http://127.0.0.1:{}\"\nservice_name = \"orders-api\"\nservice_port = 4333\n\n[environments]\ndevelopment = {{ host = \"127.0.0.1\", port = 4333 }}\n",
            registry.port
        ),
    )
    .expect("write rivet.toml");

    run_build(&app, BuildTarget::Native).expect("the fixture must build");

    let mut server = Server(
        Command::new(dir.join("generated/target/release/orders"))
            .env("HOST", "127.0.0.1")
            .env("PORT", "4333")
            .spawn()
            .expect("start the generated server"),
    );

    // The app serves, so registration did not hold startup.
    let ping = http_exchange(4333, &get_request("/ping", ""));
    assert!(ping.ends_with("{\"status\":\"pong\"}"), "{ping}");

    let registered = registry.requests();
    assert_eq!(
        registered.len(),
        1,
        "one registration request: {registered:?}"
    );
    let request = &registered[0];
    assert!(
        request.starts_with("PUT /v1/agent/service/register HTTP/1.1\r\n"),
        "the request line names the Consul agent endpoint: {request}"
    );
    assert!(
        header(request, "host") == format!("127.0.0.1:{}", registry.port),
        "the request carries the registry host: {request}"
    );
    assert_eq!(
        header(request, "content-type"),
        "application/json",
        "the registration body is JSON: {request}"
    );
    let body = request
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .unwrap_or_default();
    assert_eq!(
        header(request, "content-length"),
        body.len().to_string().as_str(),
        "the declared length matches the body: {request}"
    );
    assert!(
        body.contains("\"Name\":\"orders-api\"") && body.contains("\"Port\":4333"),
        "the registration names the service and its port: {request}"
    );

    // SIGINT runs the graceful shutdown, which leaves the registry.
    signal_and_wait(&mut server.0, "INT");
    let after = registry.requests();
    assert_eq!(
        after.len(),
        2,
        "shutdown sends one deregistration: {after:?}"
    );
    assert!(
        after[1].starts_with("PUT /v1/agent/service/deregister/orders-api HTTP/1.1\r\n"),
        "the deregistration names the service: {}",
        after[1]
    );
}

#[test]
fn a_registry_that_never_answers_does_not_hold_startup() {
    let dir = ScratchDir::new("discovery-silent");
    let app = dir.join("app.py");
    fs::write(&app, FIXTURE_APP).expect("write app.py");
    let registry = StubRegistry::silent();
    fs::write(
        dir.join("rivet.toml"),
        format!(
            "[project]\nname = \"silent\"\n\n[discovery]\nbackend = \"consul\"\nurl = \"http://127.0.0.1:{}\"\n\n[environments]\ndevelopment = {{ host = \"127.0.0.1\", port = 4334 }}\n",
            registry.port
        ),
    )
    .expect("write rivet.toml");

    run_build(&app, BuildTarget::Native).expect("the fixture must build");

    let started = Instant::now();
    let _server = Server(
        Command::new(dir.join("generated/target/release/silent"))
            .env("HOST", "127.0.0.1")
            .env("PORT", "4334")
            .spawn()
            .expect("start the generated server"),
    );

    // The client gives up on its own deadline, so the app serves anyway. A
    // hang here fails at `wait_for_port`'s 30-second limit; a client without
    // a deadline would never reach the route.
    let ping = http_exchange(4334, &get_request("/ping", ""));
    assert!(ping.ends_with("{\"status\":\"pong\"}"), "{ping}");
    assert!(
        started.elapsed() >= Duration::from_secs(4),
        "startup waited for the registry deadline, not for an answer: {:?}",
        started.elapsed()
    );
}
