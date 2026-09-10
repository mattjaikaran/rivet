use super::*;

/// A request head with the given extra headers.
fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (name, value) in pairs {
        headers.append(
            HeaderName::from_bytes(name.as_bytes()).expect("a valid header name"),
            HeaderValue::from_str(value).expect("a valid header value"),
        );
    }
    headers
}

#[test]
fn an_upgrade_request_needs_both_the_upgrade_header_and_the_connection_token() {
    assert!(is_upgrade_request(&headers(&[
        ("upgrade", "websocket"),
        ("connection", "Upgrade"),
    ])));
    assert!(is_upgrade_request(&headers(&[
        ("upgrade", "websocket"),
        ("connection", "keep-alive, Upgrade"),
    ])));
    assert!(is_upgrade_request(&headers(&[
        ("upgrade", "h2c"),
        ("connection", "upgrade"),
    ])));

    // A plain request is not an upgrade.
    assert!(!is_upgrade_request(&headers(&[(
        "connection",
        "keep-alive"
    )])));
    assert!(!is_upgrade_request(&headers(&[("upgrade", "websocket")])));
    assert!(!is_upgrade_request(&headers(&[
        ("upgrade", "websocket"),
        ("connection", "close"),
    ])));
}

#[test]
fn the_authority_is_the_origin_without_its_scheme_or_path() {
    assert_eq!(authority("http://127.0.0.1:5173"), Some("127.0.0.1:5173"));
    assert_eq!(authority("https://frontend.local/"), Some("frontend.local"));
    assert_eq!(authority("127.0.0.1:5173"), Some("127.0.0.1:5173"));
    assert_eq!(authority(""), None);
}

#[test]
fn the_reply_head_yields_its_status_and_headers() {
    let head =
        b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n";
    let (status, headers) = parse_reply_head(head).expect("a complete reply head");

    assert_eq!(status, StatusCode::SWITCHING_PROTOCOLS);
    assert_eq!(headers[header::UPGRADE], "websocket");
    assert_eq!(headers[header::CONNECTION], "Upgrade");
}

#[test]
fn a_reply_head_without_a_status_line_is_rejected() {
    assert!(parse_reply_head(b"not http\r\n\r\n").is_none());
    assert!(parse_reply_head(b"HTTP/1.1\r\n\r\n").is_none());
    assert!(parse_reply_head(b"HTTP/1.1 teapot OK\r\n\r\n").is_none());
}

#[test]
fn the_head_end_is_the_offset_past_the_blank_line() {
    assert_eq!(find_head_end(b"GET / HTTP/1.1\r\n\r\nbody"), Some(18));
    assert_eq!(find_head_end(b"\r\n\r\n"), Some(4));
    assert_eq!(find_head_end(b"GET / HTTP/1.1\r\n"), None);
}

#[test]
fn the_replayed_head_keeps_the_upgrade_headers_and_the_rewritten_target() {
    let uri: Uri = "/hmr?token=1".parse().expect("a valid URI");
    let request_headers = headers(&[
        ("host", "localhost:3000"),
        ("upgrade", "websocket"),
        ("connection", "Upgrade"),
        ("content-length", "7"),
    ]);
    let head = request_head(&Method::GET, &uri, &request_headers, "/hmr");
    let head = String::from_utf8(head).expect("the head is ASCII");

    assert!(head.starts_with("GET /hmr?token=1 HTTP/1.1\r\n"), "{head}");
    assert!(head.contains("upgrade: websocket\r\n"), "{head}");
    assert!(head.contains("connection: Upgrade\r\n"), "{head}");
    assert!(head.ends_with("\r\n\r\n"), "{head}");
    // The framing headers describe the connection the request came in on.
    assert!(!head.contains("content-length"), "{head}");
}

#[test]
fn a_head_without_a_connection_header_still_asks_for_the_upgrade() {
    let uri: Uri = "/hmr".parse().expect("a valid URI");
    let head = request_head(
        &Method::GET,
        &uri,
        &headers(&[("upgrade", "websocket")]),
        "/hmr",
    );
    let head = String::from_utf8(head).expect("the head is ASCII");

    assert!(head.contains("Connection: Upgrade\r\n"), "{head}");
}
