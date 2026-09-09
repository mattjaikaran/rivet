//! The Model Context Protocol server (phase 3, pillar 09).
//!
//! `rivet mcp` serves the Rivet tools over MCP's stdio transport so AI
//! agents can parse a DSL module, audit it, and search the phase-2 vector
//! index without shelling out. The server is transport only: the tools in
//! [`tools`] call the same parser, Gauntlet, audit, and store functions
//! the CLI commands call.
//!
//! The SDK is `rmcp` 3.x, the official Rust SDK for the Model Context
//! Protocol (github.com/modelcontextprotocol/rust-sdk, Apache-2.0). It
//! compiles at the workspace MSRV, so `rust-version` did not need a bump.
//! The choice is recorded in `docs/pillars/09-super-cli.md`.

use rmcp::ServiceExt;
pub mod tools;

use crate::diagnostic::Diagnostic;

/// Serve the MCP protocol over stdin and stdout until the client
/// disconnects.
pub fn run_stdio() -> Result<(), Diagnostic> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| {
            Diagnostic::blocker("E3011", format!("failed to start the MCP runtime: {err}"))
        })?;
    runtime.block_on(async {
        let peer = tools::RivetTools
            .serve(rmcp::transport::stdio())
            .await
            .map_err(|err| Diagnostic::blocker("E3011", format!("MCP serve error: {err}")))?;
        peer.waiting()
            .await
            .map_err(|err| Diagnostic::blocker("E3011", format!("MCP serve error: {err}")))?;
        Ok(())
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::tools::RivetTools;
    use rmcp::ServiceExt;
    use serde_json::Value;
    use std::path::PathBuf;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    /// A scratch project directory under the system temp dir.
    pub fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rivet-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    /// A fixture app module with one story-tagged route.
    fn write_fixture(dir: &std::path::Path) -> PathBuf {
        let app = dir.join("app.py");
        std::fs::write(
            &app,
            "from rivet import api\n\n@api.get(\"/ping\", stories=[\"US-1\"])\ndef ping() -> dict:\n    return {\"status\": \"pong\"}\n",
        )
        .expect("write fixture");
        app
    }

    async fn write_line<W: AsyncWriteExt + Unpin>(writer: &mut W, line: &str) {
        writer
            .write_all(format!("{line}\n").as_bytes())
            .await
            .expect("write frame");
        writer.flush().await.expect("flush frame");
    }

    async fn read_line<R: AsyncBufReadExt + Unpin>(reader: &mut R) -> Value {
        let mut line = String::new();
        reader.read_line(&mut line).await.expect("read frame");
        serde_json::from_str(line.trim()).expect("parse frame")
    }

    /// Drive the real server over an in-memory duplex and assert the MCP
    /// handshake, tool discovery, and a tool call all answer valid data.
    #[tokio::test]
    async fn probe_lists_and_calls_parse_tool() {
        let dir = scratch("mcp-probe");
        let app = write_fixture(&dir);

        let (client, server_io) = tokio::io::duplex(1 << 16);
        let (reader, mut writer) = tokio::io::split(client);
        let mut reader = BufReader::new(reader);

        let server_task = tokio::spawn(async move {
            let peer = RivetTools.serve(server_io).await.expect("server serves");
            peer.waiting().await.expect("server quits cleanly");
        });

        // MCP initialize: negotiate the protocol and announce the client.

        write_line(
            &mut writer,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"rivet-probe","version":"0.1.0"}}}"#,
        )
        .await;
        let init = read_line(&mut reader).await;
        let result = init.get("result").expect("initialize result");
        assert!(
            result.get("serverInfo").is_some(),
            "server announces itself"
        );

        write_line(
            &mut writer,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        )
        .await;

        // tools/list: the AST tool must be discoverable.
        write_line(
            &mut writer,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        )
        .await;
        let listed = read_line(&mut reader).await;
        let tools = listed
            .pointer("/result/tools")
            .and_then(Value::as_array)
            .expect("tools array");
        let names: Vec<&str> = tools
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str))
            .collect();
        for expected in [
            "parse_app",
            "audit_app",
            "session_context",
            "history",
            "vector_search",
            "explain_symptom",
        ] {
            assert!(names.contains(&expected), "{expected} listed: {names:?}");
        }

        // tools/call: parse the fixture and read the blueprint back.
        let call = format!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"parse_app","arguments":{{"app":"{}"}}}}}}"#,
            app.display()
        );
        write_line(&mut writer, &call).await;
        let called = read_line(&mut reader).await;
        let content = called
            .pointer("/result/content")
            .and_then(Value::as_array)
            .expect("content array");
        let text = content
            .first()
            .and_then(|item| item.get("text"))
            .and_then(Value::as_str)
            .expect("text content");
        let payload: Value = serde_json::from_str(text).expect("payload parses");
        assert_eq!(
            payload.get("file").and_then(Value::as_str),
            Some(app.to_str().expect("utf8 path"))
        );
        let routes = payload
            .pointer("/blueprint/routes")
            .and_then(Value::as_array)
            .expect("blueprint routes");
        assert_eq!(routes.len(), 1, "one route in the blueprint");
        assert!(payload.get("findings").is_some(), "findings present");

        // tools/call: the audit tool returns the MQI grade over the wire.
        let call = format!(
            r#"{{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{{"name":"audit_app","arguments":{{"app":"{}"}}}}}}"#,
            app.display()
        );
        write_line(&mut writer, &call).await;
        let called = read_line(&mut reader).await;
        let content = called
            .pointer("/result/content")
            .and_then(Value::as_array)
            .expect("content array");
        let text = content
            .first()
            .and_then(|item| item.get("text"))
            .and_then(Value::as_str)
            .expect("text content");
        let audit: Value = serde_json::from_str(text).expect("audit parses");
        assert_eq!(
            audit.get("grade").and_then(Value::as_str),
            Some("A+"),
            "clean fixture grades A+ over the wire"
        );

        // Closing the client ends the server cleanly.
        drop(writer);
        drop(reader);
        server_task.await.expect("server task joins");
    }
}
