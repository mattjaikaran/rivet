//! End-to-end `rivet sync` tests: a fixture app, a captured tracker payload,
//! and the diff the command reports for it.
//!
//! The source is passed in, so these tests never touch the process
//! environment and never reach the network.

use super::*;
use crate::commands::sync::config::{JiraConfig, LinearConfig};
use crate::test_support::ScratchDir;
use std::fs;
use std::path::PathBuf;

use std::sync::{Arc, Mutex};

/// A fixture app with three stories: one route carries two of them.
const FIXTURE_APP: &str = "from rivet import api\n\n@api.get(\"/ping\", stories=[\"US-001\"])\ndef ping() -> dict:\n    return {\"status\": \"pong\"}\n\n@api.post(\"/echo\", stories=[\"US-002\"])\ndef echo(request: dict) -> dict:\n    return {\"echo\": request}\n\n@api.post(\"/orders\", stories=[\"US-002\", \"US-003\"])\ndef create_order(request: dict) -> dict:\n    return {\"order\": request}\n";

/// The title the fixture's US-002 covers.
const US_002_TITLE: &str = "US-002: POST /echo; POST /orders";

/// A captured Jira enhanced-search answer: US-001 agrees, US-002 is closed,
/// and ORD-9 names a story the fixture does not declare.
const CAPTURED_JIRA: &str = "{\n  \"issues\": [\n    { \"key\": \"ORD-1\", \"fields\": { \"summary\": \"US-001: GET /ping\", \"status\": { \"statusCategory\": { \"key\": \"indeterminate\" } } } },\n    { \"key\": \"ORD-2\", \"fields\": { \"summary\": \"US-002: POST /echo; POST /orders\", \"status\": { \"statusCategory\": { \"key\": \"done\" } } } },\n    { \"key\": \"ORD-9\", \"fields\": { \"summary\": \"US-999: POST /gone\", \"status\": { \"statusCategory\": { \"key\": \"new\" } } } }\n  ]\n}";

/// A captured Jira answer that already covers every story.
const CAPTURED_JIRA_AGREEING: &str = "{\n  \"issues\": [\n    { \"key\": \"ORD-1\", \"fields\": { \"summary\": \"US-001: GET /ping\", \"status\": { \"statusCategory\": { \"key\": \"indeterminate\" } } } },\n    { \"key\": \"ORD-2\", \"fields\": { \"summary\": \"US-002: POST /echo; POST /orders\", \"status\": { \"statusCategory\": { \"key\": \"started\" } } } },\n    { \"key\": \"ORD-3\", \"fields\": { \"summary\": \"US-003: POST /orders\", \"status\": { \"statusCategory\": { \"key\": \"new\" } } } }\n  ]\n}";

/// A captured Linear GraphQL answer, shaped as the live query returns it.
const CAPTURED_LINEAR: &str = "{\n  \"data\": { \"issues\": { \"nodes\": [\n    { \"identifier\": \"ORD-1\", \"title\": \"US-001: GET /ping\", \"state\": { \"type\": \"started\" } },\n    { \"identifier\": \"ORD-2\", \"title\": \"US-002: POST /echo; POST /orders\", \"state\": { \"type\": \"completed\" } }\n  ], \"pageInfo\": { \"hasNextPage\": false, \"endCursor\": \"c1\" } } }\n}";

/// A fixture app whose story ID holds a colon, which the issue title cannot
/// carry.
const COLON_APP: &str = "from rivet import api\n\n@api.get(\"/ping\", stories=[\"EPIC:1\"])\ndef ping() -> dict:\n    return {\"status\": \"pong\"}\n";

/// A scratch project with the fixture app and one captured payload file.
fn project(name: &str, captured: &str) -> (ScratchDir, PathBuf, PathBuf) {
    let dir = ScratchDir::new(name);
    let app = dir.join("app.py");
    fs::write(&app, FIXTURE_APP).expect("write app.py");
    let payload = dir.join("payload.json");
    fs::write(&payload, captured).expect("write the captured payload");
    (dir, app, payload)
}

fn jira_provider() -> Provider {
    Provider::Jira(JiraConfig {
        base_url: "https://example.atlassian.net".to_string(),
        token: "token".to_string(),
        project: "ORD".to_string(),
    })
}

fn linear_provider() -> Provider {
    Provider::Linear(LinearConfig {
        token: "token".to_string(),
        team: "ORD".to_string(),
    })
}

/// No tracker configured, as an offline CI job runs it.
fn unconfigured() -> Result<Provider, Diagnostic> {
    Err(config::e3019("no tracker is configured", "set a tracker"))
}

/// Run the command against a captured payload, resolving the source exactly
/// as `run_sync` does.
fn run_captured(
    configured: Result<Provider, Diagnostic>,
    app: &Path,
    payload: &Path,
) -> Result<(Outcome, Diff), Diagnostic> {
    let source = source(configured, Some(payload))?;
    run_with(source, app, false)
}

#[test]
fn a_captured_payload_reports_the_expected_diff() {
    let (_dir, app, payload) = project("sync-diff", CAPTURED_JIRA);
    // With no credentials at all: the payload names its own shape.
    let (outcome, diff) = run_captured(unconfigured(), &app, &payload).expect("the diff computes");

    assert_eq!(outcome, Outcome::Diverged);
    assert_eq!(diff.missing.len(), 1);
    assert_eq!(diff.missing[0].id, "US-003");
    assert_eq!(diff.missing[0].title, "US-003: POST /orders");
    assert_eq!(diff.orphan.len(), 1);
    assert_eq!(diff.orphan[0].key, "ORD-9");
    assert_eq!(diff.drifted.len(), 1, "a closed issue drifts: {diff:?}");
    assert_eq!(diff.drifted[0].kind, issue::DriftKind::State);
    assert_eq!(diff.summary(), "1 missing, 1 orphan, 1 drifted");

    let lines = diff.lines();
    assert!(lines[0].starts_with("missing issue: US-003"), "{lines:?}");
    assert!(lines[1].starts_with("orphan issue: ORD-9"), "{lines:?}");
    assert!(lines[2].starts_with("state drift: ORD-2"), "{lines:?}");
}

#[test]
fn a_payload_that_covers_every_story_reports_an_empty_diff() {
    let (_dir, app, payload) = project("sync-agreed", CAPTURED_JIRA_AGREEING);
    let (outcome, diff) = run_captured(unconfigured(), &app, &payload).expect("the diff computes");

    assert_eq!(outcome, Outcome::Agreed, "{diff:?}");
    assert!(diff.is_empty(), "{diff:?}");
    assert_eq!(diff.summary(), "the blueprint and the tracker agree");
}

#[test]
fn a_linear_payload_drives_the_same_diff() {
    let (_dir, app, payload) = project("sync-linear", CAPTURED_LINEAR);
    // A configured Linear tracker, so the provider decides the shape.
    let (outcome, diff) =
        run_captured(Ok(linear_provider()), &app, &payload).expect("the diff computes");

    assert_eq!(outcome, Outcome::Diverged);
    assert_eq!(diff.missing.len(), 1);
    assert_eq!(diff.missing[0].id, "US-003");
    assert_eq!(diff.drifted.len(), 1);
    assert_eq!(diff.drifted[0].kind, issue::DriftKind::State);
    assert!(diff.orphan.is_empty());
}

/// A stub tracker: it answers a list of canned JSON bodies in order, then
/// repeats the last one, and records every request it received.
///
/// This is the only oracle for the hand-rolled client, so the tests assert
/// the requests the client sent, not just the diff it computed.
struct Tracker {
    /// The port the stub listens on.
    port: u16,
    /// Every request, as its first line, in arrival order.
    seen: Arc<Mutex<Vec<String>>>,
}

impl Tracker {
    /// Start a stub that answers `bodies` in order, repeating the last one,
    /// then answers `status` for every later request.
    fn start(bodies: Vec<String>, status: u16) -> Tracker {
        use std::io::Write;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind the stub tracker");
        let port = listener.local_addr().expect("stub address").port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&seen);
        std::thread::spawn(move || {
            let mut remaining = bodies.into_iter();
            let mut last = String::new();
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let request = read_one_request(&mut stream);
                if let Ok(mut recorded) = recorded.lock() {
                    recorded.push(request);
                }
                let answering = if let Some(body) = remaining.next() {
                    last = body;
                    200
                } else {
                    status
                };
                let response = format!(
                    "HTTP/1.1 {answering} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{last}",
                    last.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        Tracker { port, seen }
    }

    /// A stub that answers one Jira search page with `issues`, then the last
    /// body, with `200`, for every later request.
    fn jira(issues: &str) -> Tracker {
        Tracker::start(vec![format!("{{\"issues\":[{issues}]}}")], 200)
    }

    /// The requests the stub received so far.
    fn requests(&self) -> Vec<String> {
        self.seen.lock().expect("the stub records").clone()
    }
}

/// One Jira issue object, with `closed` choosing its status category.
fn jira_issue(key: &str, title: &str, closed: bool) -> String {
    let category = if closed { "done" } else { "new" };
    format!(
        "{{\"key\":\"{key}\",\"fields\":{{\"summary\":\"{title}\",\"status\":{{\"statusCategory\":{{\"key\":\"{category}\"}}}}}}}}"
    )
}

/// Read one request and return its first line: the method and the path.
fn read_one_request(stream: &mut std::net::TcpStream) -> String {
    use std::io::Read;
    let mut buffer = [0u8; 2048];
    let read = stream.read(&mut buffer).unwrap_or(0);
    String::from_utf8_lossy(&buffer[..read])
        .lines()
        .next()
        .unwrap_or_default()
        .to_string()
}

/// A provider pointed at a stub on `port`, with its own project name so the
/// JQL is unique per test.
fn stub_provider(port: u16) -> Provider {
    Provider::Jira(JiraConfig {
        base_url: format!("http://127.0.0.1:{port}"),
        token: "token".to_string(),
        project: "ORD".to_string(),
    })
}

#[test]
fn a_paged_tracker_is_read_to_the_end() {
    let first = format!(
        "{{\"issues\":[{}],\"nextPageToken\":\"tok-1\"}}",
        jira_issue("ORD-1", "US-001: GET /ping", false)
    );
    let second = format!(
        "{{\"issues\":[{}]}}",
        jira_issue("ORD-3", "US-003: POST /orders", false)
    );
    let tracker = Tracker::start(vec![first, second], 200);

    let issues = read_tracker(&stub_provider(tracker.port)).expect("both pages read");
    assert_eq!(issues.len(), 2, "the second page contributes: {issues:?}");
    assert_eq!(issues[1].title, "US-003: POST /orders");

    let requests = tracker.requests();
    assert_eq!(requests.len(), 2, "both pages were requested: {requests:?}");
    assert!(
        requests[0].contains("GET /rest/api/3/search/jql?")
            && !requests[0].contains("nextPageToken"),
        "the first request asks for the first page: {}",
        requests[0]
    );
    assert!(
        requests[1].contains("nextPageToken=tok-1"),
        "the second request carries the token the first answer named: {}",
        requests[1]
    );
}

#[test]
fn a_page_token_cycle_is_bounded() {
    // The stub alternates two tokens, so a consecutive-repeat guard would
    // never fire and the issue list would grow without bound.
    let page = |token: &str| {
        format!(
            "{{\"issues\":[{}],\"nextPageToken\":\"{token}\"}}",
            jira_issue("ORD-1", "US-001: GET /ping", false)
        )
    };
    let tracker = Tracker::start(vec![page("tok-a"), page("tok-b")], 200);

    let error = read_tracker(&stub_provider(tracker.port)).expect_err("the cycle is bounded");
    assert_eq!(error.error_code, "E3020");
    assert!(
        error.message.contains("paging"),
        "the message names the paging failure: {}",
        error.message
    );
    assert!(
        tracker.requests().len() <= 64,
        "the cap stops the loop instead of paging forever"
    );
}

#[test]
fn a_page_token_that_never_converges_is_reported() {
    // The stub repeats one token forever, so a client that keeps paging
    // would never finish.
    let page = format!(
        "{{\"issues\":[{}],\"nextPageToken\":\"tok-1\"}}",
        jira_issue("ORD-1", "US-001: GET /ping", false)
    );
    let tracker = Tracker::start(vec![page], 200);

    let error = read_tracker(&stub_provider(tracker.port)).expect_err("the token repeats");
    assert_eq!(error.error_code, "E3020");
    assert!(error.message.contains("tok-1"), "{}", error.message);
    assert!(!error.suggested_fix.is_empty());
}

#[test]
fn apply_creates_the_missing_issues_and_then_agrees() {
    let dir = ScratchDir::new("sync-apply");
    let app = dir.join("app.py");
    fs::write(&app, FIXTURE_APP).expect("write app.py");

    // The tracker holds US-001 and US-002 exactly, so only US-003 is
    // missing; the create calls then answer one new key each.
    let search = format!(
        "{{\"issues\":[{},{}]}}",
        jira_issue("ORD-1", "US-001: GET /ping", false),
        jira_issue("ORD-2", US_002_TITLE, false),
    );
    let tracker = Tracker::start(vec![search, "{\"key\":\"ORD-3\"}".to_string()], 200);

    let source = Source::Tracker(stub_provider(tracker.port));
    let (outcome, diff) = run_with(source, &app, true).expect("the run succeeds");

    // Every story is tracked, so the run agrees and the command exits 0.
    assert_eq!(outcome, Outcome::Agreed, "{diff:?}");
    assert!(diff.is_empty(), "{diff:?}");

    let requests = tracker.requests();
    assert_eq!(requests.len(), 2, "one read, one create: {requests:?}");
    assert!(
        requests[1].starts_with("POST /rest/api/3/issue "),
        "the created issue goes to the issue endpoint: {}",
        requests[1]
    );
}

#[test]
fn a_rejected_creation_is_reported_as_a_write_failure() {
    let dir = ScratchDir::new("sync-apply-rejected");
    let app = dir.join("app.py");
    fs::write(&app, FIXTURE_APP).expect("write app.py");

    // The search answers no issues, and the create answers no key.
    let tracker = Tracker::start(vec!["{\"issues\":[]}".to_string(), "{}".to_string()], 200);
    let source = Source::Tracker(stub_provider(tracker.port));

    let error = run_with(source, &app, true).expect_err("the create answer names no key");
    assert_eq!(error.error_code, "E3020");
    assert!(!error.suggested_fix.is_empty());
}

#[test]
fn a_missing_captured_payload_is_reported_with_a_fix() {
    let (_dir, app, _payload) = project("sync-no-payload", CAPTURED_JIRA);
    let missing = app.with_file_name("absent.json");
    let error = run_captured(Ok(jira_provider()), &app, &missing).expect_err("the file is absent");
    assert_eq!(error.error_code, "E3020");
    assert!(!error.suggested_fix.is_empty());
}

#[test]
fn a_payload_the_provider_cannot_read_is_reported_with_a_fix() {
    let (_dir, app, payload) = project("sync-bad-payload", "{\"unexpected\":true}");
    let error = run_captured(Ok(jira_provider()), &app, &payload).expect_err("the shape is wrong");
    assert_eq!(error.error_code, "E3020");
    assert!(!error.suggested_fix.is_empty());
}

#[test]
fn a_story_id_the_title_cannot_carry_fails_the_run() {
    let dir = ScratchDir::new("sync-colon");
    let app = dir.join("app.py");
    fs::write(&app, COLON_APP).expect("write app.py");
    let payload = dir.join("payload.json");
    fs::write(&payload, CAPTURED_JIRA).expect("write the payload");

    let error = run_captured(unconfigured(), &app, &payload)
        .expect_err("a colon cannot go into an issue title");
    assert_eq!(error.error_code, "E3023");
    assert!(error.message.contains("EPIC:1"));
}
