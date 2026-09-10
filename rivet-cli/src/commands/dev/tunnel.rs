//! The upgrade tunnel that carries the frontend dev server's HMR socket.
//!
//! A dev server's HMR client connects to the page origin, which is the proxy
//! port, and asks for `Upgrade: websocket`. The proxy cannot answer that
//! handshake itself, and an HTTP client cannot tunnel: it must replay the
//! request head on a raw connection to the frontend, relay the `101`, and
//! then copy bytes in both directions until either side closes.

use super::DevTopology;
use super::routing::upstream_path;
use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri, header};
use axum::response::Response;
use hyper::upgrade::{OnUpgrade, Upgraded};
use hyper_util::rt::TokioIo;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// How long the frontend dev server has to accept the connection and answer
/// the upgrade request.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// The largest reply head the proxy reads from the frontend.
const MAX_HEAD: usize = 16 * 1024;

/// Whether the request asks to upgrade the connection, the way hyper reads
/// it: an `Upgrade` header plus an `upgrade` token in `Connection`.
pub(crate) fn is_upgrade_request(headers: &HeaderMap) -> bool {
    headers.contains_key(header::UPGRADE)
        && headers
            .get_all(header::CONNECTION)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .any(|value| {
                value
                    .split(',')
                    .any(|token| token.trim().eq_ignore_ascii_case("upgrade"))
            })
}

/// Forward an upgrade request to the frontend dev server and hand the
/// upgraded connection to the client.
pub(crate) async fn tunnel(topology: &DevTopology, origin: &str, request: Request) -> Response {
    let (mut parts, _body) = request.into_parts();
    let on_upgrade = parts.extensions.remove::<OnUpgrade>();
    let Some(on_upgrade) = on_upgrade else {
        return super::bad_gateway("the connection cannot be upgraded to the frontend dev server");
    };
    let Some(authority) = authority(origin) else {
        return super::bad_gateway(&format!("{origin} is not a usable upstream origin"));
    };

    let mut frontend = match connect(authority).await {
        Ok(stream) => stream,
        Err(detail) => return super::bad_gateway(&detail),
    };
    let head = request_head(
        &parts.method,
        &parts.uri,
        &parts.headers,
        upstream_path(topology, parts.uri.path()),
    );
    if let Err(err) = frontend.write_all(&head).await {
        return super::bad_gateway(&format!("{origin} did not read the upgrade request: {err}"));
    }
    let (status, headers, rest) = match read_reply(&mut frontend).await {
        Ok(reply) => reply,
        Err(detail) => return super::bad_gateway(&detail),
    };
    if status != StatusCode::SWITCHING_PROTOCOLS {
        // The frontend refused the upgrade; relay its answer verbatim.
        return relayed(status, headers, rest);
    }

    tokio::spawn(async move {
        match on_upgrade.await {
            Ok(upgraded) => relay_both_ways(upgraded, frontend, rest).await,
            Err(err) => eprintln!("rivet dev: the client upgrade failed: {err}"),
        }
    });
    switching_protocols(headers)
}

/// Connect to the frontend dev server, or report why the tunnel cannot open.
async fn connect(authority: &str) -> Result<TcpStream, String> {
    match tokio::time::timeout(HANDSHAKE_TIMEOUT, TcpStream::connect(authority)).await {
        Ok(Ok(stream)) => Ok(stream),
        Ok(Err(err)) => Err(format!("cannot reach {authority}: {err}")),
        Err(_) => Err(format!("{authority} did not accept the connection in time")),
    }
}

/// Copy bytes in both directions between the client and the frontend.
async fn relay_both_ways(upgraded: Upgraded, mut frontend: TcpStream, pending: Vec<u8>) {
    let mut client = TokioIo::new(upgraded);
    // The frontend may send frames in the same packet as its `101` head.
    if !pending.is_empty() && client.write_all(&pending).await.is_err() {
        return;
    }
    let _ = tokio::io::copy_bidirectional(&mut client, &mut frontend).await;
}

/// The request head to replay on the frontend connection.
fn request_head(method: &Method, uri: &Uri, headers: &HeaderMap, path: &str) -> Vec<u8> {
    let mut head = Vec::with_capacity(512);
    let target = match uri.query() {
        Some(query) => format!("{path}?{query}"),
        None => path.to_string(),
    };
    head.extend_from_slice(format!("{method} {target} HTTP/1.1\r\n").as_bytes());
    let mut wrote_upgrade = false;
    for (name, value) in headers {
        if name == header::CONTENT_LENGTH || name == header::TRANSFER_ENCODING {
            continue;
        }
        if name == header::CONNECTION {
            wrote_upgrade = true;
        }
        head.extend_from_slice(name.as_str().as_bytes());
        head.extend_from_slice(b": ");
        head.extend_from_slice(value.as_bytes());
        head.extend_from_slice(b"\r\n");
    }
    if !wrote_upgrade {
        head.extend_from_slice(b"Connection: Upgrade\r\n");
    }
    head.extend_from_slice(b"\r\n");
    head
}

/// Read the reply head from the frontend, plus any bytes that follow it.
async fn read_reply(stream: &mut TcpStream) -> Result<(StatusCode, HeaderMap, Vec<u8>), String> {
    let mut buffer = Vec::with_capacity(1024);
    loop {
        if let Some(end) = find_head_end(&buffer) {
            let reply = parse_reply_head(&buffer[..end])
                .ok_or_else(|| "the frontend dev server sent a malformed reply head".to_string())?;
            return Ok((reply.0, reply.1, buffer[end..].to_vec()));
        }
        if buffer.len() >= MAX_HEAD {
            return Err("the frontend dev server sent no complete reply head".to_string());
        }
        let mut chunk = [0_u8; 1024];
        let read = match tokio::time::timeout(HANDSHAKE_TIMEOUT, stream.read(&mut chunk)).await {
            Ok(Ok(read)) => read,
            Ok(Err(err)) => return Err(format!("cannot read the frontend reply: {err}")),
            Err(_) => return Err("the frontend dev server did not answer in time".to_string()),
        };
        if read == 0 {
            return Err("the frontend dev server closed the connection".to_string());
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
}

/// The offset just past the `\r\n\r\n` that ends an HTTP head.
fn find_head_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|start| start + 4)
}

/// The status and headers of an HTTP reply head, as `head` ends with the
/// blank line.
fn parse_reply_head(head: &[u8]) -> Option<(StatusCode, HeaderMap)> {
    let text = std::str::from_utf8(head).ok()?;
    let mut lines = text.split("\r\n");
    let status = lines.next()?.split_whitespace().nth(1)?;
    let status = StatusCode::from_u16(status.parse().ok()?).ok()?;
    let mut headers = HeaderMap::new();
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(name.trim().as_bytes()),
            HeaderValue::from_bytes(value.trim().as_bytes()),
        ) {
            headers.append(name, value);
        }
    }
    Some((status, headers))
}

/// The `101` the client receives once the frontend accepted the upgrade.
fn switching_protocols(headers: HeaderMap) -> Response {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::SWITCHING_PROTOCOLS;
    copy_headers(&mut response, &headers, |_| false);
    response
}

/// A non-`101` frontend reply, relayed with the bytes that followed its head.
///
/// The relayed reply is a fresh HTTP message over a connection the client
/// keeps using, so it drops the hop-by-hop headers as well as the framing
/// ones. The proxy's own body length replaces `content-length`.
fn relayed(status: StatusCode, headers: HeaderMap, rest: Vec<u8>) -> Response {
    let mut response = Response::new(Body::from(rest));
    *response.status_mut() = status;
    copy_headers(&mut response, &headers, super::is_hop_by_hop);
    response
}

/// Copy the frontend's reply headers onto the client response, skipping the
/// ones `skip` rejects. The 101 keeps the upgrade headers it needs; the
/// framing headers describe the head alone, so the tunnel never relays them.
fn copy_headers(response: &mut Response, headers: &HeaderMap, skip: impl Fn(&HeaderName) -> bool) {
    for (name, value) in headers {
        if name == header::CONTENT_LENGTH || name == header::TRANSFER_ENCODING || skip(name) {
            continue;
        }
        response.headers_mut().append(name.clone(), value.clone());
    }
}

/// The `host:port` of an upstream origin, or `None` when it has no authority.
fn authority(origin: &str) -> Option<&str> {
    let rest = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
        .unwrap_or(origin);
    let authority = rest.split('/').next().unwrap_or_default();
    if authority.is_empty() {
        None
    } else {
        Some(authority)
    }
}
#[cfg(test)]
mod tests;
