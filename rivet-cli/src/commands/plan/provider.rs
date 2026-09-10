//! OpenAI-compatible provider for `rivet /plan` (phase 3, pillar 09).
//!
//! The provider speaks the `/chat/completions` protocol, so any
//! OpenAI-compatible server works: the hosted OpenAI API by default, or a
//! local server (Ollama, llama.cpp, vLLM) via `RIVET_PLAN_BASE_URL`.
//!
//! Configuration comes from the environment:
//!
//! - `RIVET_PLAN_BASE_URL` — server root; default `https://api.openai.com/v1`
//! - `RIVET_PLAN_API_KEY` — required for the default cloud provider,
//!   optional when the user set a local base URL
//! - `RIVET_PLAN_MODEL` — model name; default `gpt-4o-mini`
//!
//! Prompt building, request-body assembly, and response parsing are small
//! pure functions so the unit tests exercise them offline; only [`chat`]
//! touches the network. Every failure in this module is an E3012 diagnostic
//! with a concrete fix.

use crate::diagnostic::Diagnostic;
use serde_json::{Map, Value};
use std::time::Duration;

/// Server root when the user did not point at a local provider.
const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
/// Default model: small and fast, so a plan round trips quickly.
const DEFAULT_MODEL: &str = "gpt-4o-mini";
/// Provider calls must answer within this window.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// Resolved provider settings from the environment.
pub struct ProviderConfig {
    base_url: String,
    model: String,
    api_key: Option<String>,
}

/// What the provider agreed to do: a short spec and the full module source.
#[derive(Debug)]
pub struct PlanResponse {
    /// Human summary of the planned change, written to SPEC.md.
    pub spec: String,
    /// The complete replacement Python DSL module.
    pub module: String,
}

/// Read the provider configuration from the environment.
///
/// A missing API key is an error only when the base URL is the default cloud
/// provider; a user-set base URL implies a local server that needs no key.
pub fn from_env() -> Result<ProviderConfig, Diagnostic> {
    let base_set = std::env::var_os("RIVET_PLAN_BASE_URL").is_some();
    let base_url =
        std::env::var("RIVET_PLAN_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
    let model = std::env::var("RIVET_PLAN_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string());
    let api_key = std::env::var("RIVET_PLAN_API_KEY").ok();
    if api_key.is_none() && !base_set {
        return Err(Diagnostic::blocker(
            "E3012",
            "RIVET_PLAN_API_KEY is not set and RIVET_PLAN_BASE_URL still points at the default cloud provider",
            "export RIVET_PLAN_API_KEY=\"<key>\" for a cloud provider, or set RIVET_PLAN_BASE_URL to a local OpenAI-compatible server such as http://localhost:11434/v1, then run rivet /plan again",
        ));
    }
    Ok(ProviderConfig {
        base_url,
        model,
        api_key,
    })
}

/// Generate a replacement module for a story against the current context.
pub async fn generate(
    cfg: &ProviderConfig,
    story: &str,
    context: &str,
) -> Result<PlanResponse, Diagnostic> {
    chat(cfg, story, context, None).await
}

/// Generate again with the diagnostics of the failed attempt appended to the
/// user message so the model can repair its own module.
pub async fn generate_with_feedback(
    cfg: &ProviderConfig,
    story: &str,
    context: &str,
    feedback: &str,
) -> Result<PlanResponse, Diagnostic> {
    chat(cfg, story, context, Some(feedback)).await
}

/// POST `/chat/completions` and parse the answer.
async fn chat(
    cfg: &ProviderConfig,
    story: &str,
    context: &str,
    feedback: Option<&str>,
) -> Result<PlanResponse, Diagnostic> {
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|err| {
            e3012(
                format!("failed to build the HTTP client: {err}"),
                "retry rivet /plan; report the error if it keeps happening",
            )
        })?;
    let endpoint = format!("{}/chat/completions", cfg.base_url.trim_end_matches('/'));

    let mut request = client
        .post(&endpoint)
        .json(&request_body(cfg, story, context, feedback));
    if let Some(key) = &cfg.api_key {
        request = request.bearer_auth(key);
    }

    let response = request.send().await.map_err(|err| {
        e3012(
            format!("the provider request to {endpoint} failed: {err}"),
            "check that the provider is reachable and RIVET_PLAN_BASE_URL is correct, then run rivet /plan again",
        )
    })?;
    if !response.status().is_success() {
        let status = response.status();
        let detail = response.text().await.unwrap_or_default();
        return Err(e3012(
            format!("the provider returned HTTP {status}: {detail}"),
            "check RIVET_PLAN_API_KEY and RIVET_PLAN_BASE_URL, then run rivet /plan again",
        ));
    }
    let payload: Value = response.json().await.map_err(|err| {
        e3012(
            format!("the provider returned a non-JSON response: {err}"),
            "check that RIVET_PLAN_MODEL names a chat model, then run rivet /plan again",
        )
    })?;
    let content = payload["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| {
            e3012(
                "the provider response has no choices[0].message.content; the model may have refused the request",
                "check RIVET_PLAN_API_KEY and RIVET_PLAN_MODEL, then run rivet /plan again",
            )
        })?;
    parse_response(content)
}

/// The request body for `/chat/completions`, assembled by hand from
/// [`Value`] constructors (the `json!` macro is banned in this workspace).
fn request_body(cfg: &ProviderConfig, story: &str, context: &str, feedback: Option<&str>) -> Value {
    let mut body = Map::new();
    body.insert("model".into(), Value::String(cfg.model.clone()));
    body.insert("temperature".into(), Value::from(0));
    let messages: Vec<Value> = vec![
        role_message("system", system_prompt()),
        role_message("user", &user_prompt(story, context, feedback)),
    ];
    body.insert("messages".into(), Value::Array(messages));
    Value::Object(body)
}

fn role_message(role: &str, content: &str) -> Value {
    let mut message = Map::new();
    message.insert("role".into(), Value::String(role.into()));
    message.insert("content".into(), Value::String(content.into()));
    Value::Object(message)
}

/// The user message: the story, the rendered module context, and the
/// diagnostics of the failed attempt when this is a repair round.
fn user_prompt(story: &str, context: &str, feedback: Option<&str>) -> String {
    let mut out = format!("Story: {story}\n\nContext:\n{context}\n");
    if let Some(feedback) = feedback {
        out.push_str("\nThe previous attempt failed with these diagnostics:\n");
        out.push_str(feedback);
        out.push_str(
            "\nFix every diagnostic; the corrected module must parse and pass the Gauntlet.\n",
        );
    }
    out.push_str("\nReturn ONLY one JSON object with two keys: \"spec\" (a short summary of the change) and \"module\" (the COMPLETE replacement python module).");
    out
}

/// The system prompt: a compact Rivet DSL grammar reference.
///
/// This is the only grammar source the provider sees, so keep it in sync with
/// the supported-surface list in `parser/python.rs`.
fn system_prompt() -> &'static str {
    "You write Python modules in the Rivet DSL, which transpiles to an axum Rust server. \
     Write ONLY code inside the documented subset; anything outside it fails the build.\n\
     \n\
     Subset:\n\
     - The module starts with `from rivet import api`. No other imports and no \
     module-level statements.\n\
     - A route is a top-level function decorated with @api.<method>(\"/path\", stories=[...]). \
     Methods: get, post, put, delete, patch, options, head.\n\
     - Every route MUST tag the story IDs named in the story, for example \
     @api.get(\"/health\", stories=[\"US-42\"]).\n\
     - Route paths start with `/` and contain no {placeholders} yet.\n\
     - A handler has a return annotation and at most ONE request parameter. \
     Annotations are only dict, str, bool, int, float, list[T], DTO class names, or \
     None for `-> None`. No defaults, *args, or **kwargs.\n\
     - The handler body is a SINGLE return over the supported expression subset: \
     literals (None, True, False, integers, floats, strings, lists, dictionaries), a \
     reference to the request parameter, or one DTO constructor call such as \
     OrderResponse(status=\"ok\"). No calls, attribute access, arithmetic, control \
     flow, comprehensions, or f-strings.\n\
     - A DTO is an annotation-only class: `class OrderCreate:` whose body holds typed \
     fields (`sku: str`) and nothing else. Every field is annotated.\n\
     - No helper functions, runtime classes, or foreign decorators.\n\
     \n\
     Keep every existing route and DTO from the context module unless the story says \
     otherwise."
}

/// Parse the assistant message content into a [`PlanResponse`].
///
/// The model may wrap the JSON in ``` fences; strip them, take the text from
/// the first `{` to the last `}`, and decode it. Shape failures become E3012.
fn parse_response(content: &str) -> Result<PlanResponse, Diagnostic> {
    parse_content(content).map_err(|reason| {
        e3012(
            format!("the provider returned an unparseable response: {reason}"),
            "check that RIVET_PLAN_MODEL names a chat model that follows instructions, then run rivet /plan again",
        )
    })
}

/// Pure JSON extraction used by [`parse_response`].
fn parse_content(content: &str) -> Result<PlanResponse, String> {
    let stripped = strip_fences(content);
    let start = stripped
        .find('{')
        .ok_or_else(|| "no JSON object found in the response".to_string())?;
    let end = stripped
        .rfind('}')
        .ok_or_else(|| "no closing brace found in the response".to_string())?;
    if end < start {
        return Err("the JSON object is empty or malformed".to_string());
    }
    let value: Value = serde_json::from_str(&stripped[start..=end])
        .map_err(|err| format!("invalid JSON: {err}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| "the payload is not a JSON object".to_string())?;
    let module = object
        .get("module")
        .and_then(Value::as_str)
        .ok_or_else(|| "the JSON object has no string \"module\" key".to_string())?;
    let spec = object
        .get("spec")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Ok(PlanResponse {
        spec: spec.to_string(),
        module: module.to_string(),
    })
}

/// Remove an optional ``` fence and its language tag around the JSON.
fn strip_fences(content: &str) -> &str {
    let mut text = content.trim();
    if let Some(rest) = text.strip_prefix("```") {
        text = match rest.find('\n') {
            Some(index) => &rest[index + 1..],
            None => rest,
        };
    }
    if let Some(rest) = text.strip_suffix("```") {
        text = rest;
    }
    text.trim()
}

fn e3012(message: impl Into<String>, fix: impl Into<String>) -> Diagnostic {
    Diagnostic::blocker("E3012", message, fix)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A provider config for offline tests: never read the environment.
    fn test_config() -> ProviderConfig {
        ProviderConfig {
            base_url: "http://localhost:11434/v1".to_string(),
            model: "local-test-model".to_string(),
            api_key: None,
        }
    }

    #[test]
    fn fenced_provider_response_parses_to_the_module() {
        let content = "```json\n{\"spec\": \"add a health route\", \
            \"module\": \"from rivet import api\\n\\n@api.get(\\\"/health\\\", \
            stories=[\\\"US-42\\\"])\\ndef health() -> dict:\\n    return \
            {\\\"status\\\": \\\"ok\\\"}\\n\"}\n```";
        let response = parse_response(content).expect("fenced JSON parses");
        assert_eq!(response.spec, "add a health route");
        assert!(
            response.module.contains("@api.get(\"/health\""),
            "{}",
            response.module
        );
        assert!(response.module.contains("US-42"), "{}", response.module);
    }

    #[test]
    fn malformed_response_is_an_e3012_diagnostic() {
        let error = parse_response("the model said: sorry, no JSON today")
            .expect_err("prose is not a response");
        assert_eq!(error.error_code, "E3012");
        assert!(!error.suggested_fix.is_empty());
    }

    #[test]
    fn request_body_carries_the_story_and_the_model() {
        let cfg = test_config();
        let body = request_body(&cfg, "add a health route US-42", "# context", None);
        let text = body.to_string();
        assert!(text.contains("add a health route US-42"), "{text}");
        assert!(text.contains("\"model\":\"local-test-model\""), "{text}");
        assert!(text.contains("\"role\":\"system\""), "{text}");
        assert!(text.contains("\"role\":\"user\""), "{text}");
    }

    #[test]
    fn feedback_round_appends_the_diagnostics_to_the_user_message() {
        let cfg = test_config();
        let body = request_body(&cfg, "fix the route", "# context", Some("\"E2045\""));
        let text = body.to_string();
        assert!(text.contains("previous attempt failed"), "{text}");
        assert!(text.contains("E2045"), "{text}");
    }
}
