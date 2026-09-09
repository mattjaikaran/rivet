//! Structured, machine-readable build diagnostics.
//!
//! Every failure the transpiler can raise becomes a [`Diagnostic`]. The CLI
//! prints it to stderr as one JSON object so AI agents can act on it, and as a
//! one-line human summary. The shape follows the Gauntlet error contract from
//! the design docs (`docs/pillars/07-the-gauntlet.md`).

use serde_json::Value;

/// Severity of a diagnostic. Phase 0 reports only blockers; warnings arrive
/// with the Gauntlet, which will extend this enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Blocker,
}

impl Severity {
    fn as_str(self) -> &'static str {
        match self {
            Severity::Blocker => "blocker",
        }
    }
}

/// A structured diagnostic with a stable error code.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub error_code: String,
    pub severity: Severity,
    pub message: String,
    pub file: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub suggested_fix: Option<String>,
    pub ast_path: Option<String>,
}

impl Diagnostic {
    /// Create a blocker with a stable error code.
    pub fn blocker(error_code: &str, message: impl Into<String>) -> Self {
        Self {
            error_code: error_code.to_string(),
            severity: Severity::Blocker,
            message: message.into(),
            file: None,
            line: None,
            column: None,
            suggested_fix: None,
            ast_path: None,
        }
    }

    /// Attach source location and a suggested fix.
    pub fn located(
        mut self,
        file: impl Into<String>,
        line: usize,
        fix: Option<impl Into<String>>,
    ) -> Self {
        self.file = Some(file.into());
        self.line = Some(line);
        self.suggested_fix = fix.map(Into::into);
        self
    }

    /// One-line human summary, e.g. `E1001 at app.py:12: missing type hint`.
    pub fn summary(&self) -> String {
        match (&self.file, self.line) {
            (Some(file), Some(line)) => {
                format!("{} at {}:{}: {}", self.error_code, file, line, self.message)
            }
            _ => format!("{}: {}", self.error_code, self.message),
        }
    }

    /// JSON payload for agents.
    ///
    /// Built from [`serde_json::Value`] constructors only, which are
    /// infallible, so this method cannot panic (the project bans `unwrap` and
    /// `expect`).
    pub fn to_json(&self) -> String {
        let mut object = serde_json::Map::new();
        object.insert("error_code".into(), Value::String(self.error_code.clone()));
        object.insert(
            "severity".into(),
            Value::String(self.severity.as_str().into()),
        );
        object.insert("message".into(), Value::String(self.message.clone()));
        if let Some(file) = &self.file {
            object.insert("file".into(), Value::String(file.clone()));
        }
        if let Some(line) = self.line {
            object.insert("line".into(), Value::from(line));
        }
        if let Some(column) = self.column {
            object.insert("column".into(), Value::from(column));
        }
        if let Some(fix) = &self.suggested_fix {
            object.insert("suggested_fix".into(), Value::String(fix.clone()));
        }
        if let Some(path) = &self.ast_path {
            object.insert("ast_path".into(), Value::String(path.clone()));
        }
        Value::Object(object).to_string()
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.summary())
    }
}

impl std::error::Error for Diagnostic {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_payload_is_parseable_and_complete() {
        let diagnostic = Diagnostic::blocker("E1001", "missing type hint").located(
            "app.py",
            12,
            Some("add a return type annotation"),
        );
        let json: Value = serde_json::from_str(&diagnostic.to_json()).expect("payload must parse");
        assert_eq!(json["error_code"], "E1001");
        assert_eq!(json["severity"], "blocker");
        assert_eq!(json["file"], "app.py");
        assert_eq!(json["line"], 12);
        assert_eq!(json["suggested_fix"], "add a return type annotation");
    }
}
