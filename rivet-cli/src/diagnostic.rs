//! Structured, machine-readable build diagnostics.
//!
//! Every failure the transpiler can raise becomes a [`Diagnostic`]. The CLI
//! prints it to stderr as one JSON object so AI agents can act on it, and as a
//! one-line human summary. The shape follows the Gauntlet error contract from
//! the design docs (`docs/pillars/07-the-gauntlet.md`).
//!
//! Error-code ranges: the parser front end owns `E1xxx`, the generator and
//! the Gauntlet own `E2xxx`. The Gauntlet rules (see
//! [`crate::gauntlet`]) report `E2042`-`E2046`.

use serde::de::Error as _;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

/// Severity of a diagnostic.
///
/// Phase 0 reported only blockers. The Gauntlet added warnings: a warning
/// prints to stderr and lets the build continue, a blocker stops it. The
/// `[gauntlet]` config maps rule outcomes to these severities, so the same
/// enum deserializes from `rivet.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Report and continue.
    Warning,
    /// Stop the build with this finding.
    Blocker,
}

impl Severity {
    fn as_str(self) -> &'static str {
        match self {
            Severity::Warning => "warning",
            Severity::Blocker => "blocker",
        }
    }

    /// Parse a config word. `rivet.toml` writes `warn`/`warning` or
    /// `block`/`blocker`; anything else is an error naming the value.
    pub fn from_config_word(word: &str) -> Result<Severity, String> {
        match word {
            "warn" | "warning" => Ok(Severity::Warning),
            "block" | "blocker" => Ok(Severity::Blocker),
            other => Err(format!(
                "`{other}` is not a severity; use `warn` or `block`"
            )),
        }
    }
}

impl<'de> Deserialize<'de> for Severity {
    fn deserialize<D>(deserializer: D) -> Result<Severity, D::Error>
    where
        D: Deserializer<'de>,
    {
        let word = String::deserialize(deserializer)?;
        Severity::from_config_word(&word).map_err(D::Error::custom)
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
        Self::new(error_code, Severity::Blocker, message)
    }

    fn new(error_code: &str, severity: Severity, message: impl Into<String>) -> Self {
        Self {
            error_code: error_code.to_string(),
            severity,
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
        self.to_json_value().to_string()
    }

    /// The diagnostic as a JSON value, for embedding in larger payloads.
    ///
    /// Built from [`serde_json::Value`] constructors only, which are
    /// infallible, so this method cannot panic (the project bans `unwrap` and
    /// `expect`).
    pub fn to_json_value(&self) -> Value {
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
        Value::Object(object)
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.summary())
    }
}

impl From<Diagnostic> for Vec<Diagnostic> {
    fn from(diagnostic: Diagnostic) -> Self {
        vec![diagnostic]
    }
}

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

    #[test]
    fn warning_severity_serializes_in_the_payload() {
        let diagnostic = Diagnostic {
            error_code: "E2044".to_string(),
            severity: Severity::Warning,
            message: "helper is never called".to_string(),
            file: None,
            line: None,
            column: None,
            suggested_fix: None,
            ast_path: None,
        };
        let json: Value = serde_json::from_str(&diagnostic.to_json()).expect("payload must parse");
        assert_eq!(json["severity"], "warning");
        assert_eq!(json["error_code"], "E2044");
    }

    #[test]
    fn severity_parses_config_words() {
        assert_eq!(Severity::from_config_word("warn"), Ok(Severity::Warning));
        assert_eq!(Severity::from_config_word("warning"), Ok(Severity::Warning));
        assert_eq!(Severity::from_config_word("block"), Ok(Severity::Blocker));
        assert_eq!(Severity::from_config_word("blocker"), Ok(Severity::Blocker));
        assert!(Severity::from_config_word("loud").is_err());
    }

    #[test]
    fn severity_deserializes_from_toml_words() {
        #[derive(serde::Deserialize)]
        struct Holder {
            severity: Severity,
        }
        let holder: Holder = toml::from_str(r#"severity = "warn""#).expect("word must parse");
        assert_eq!(holder.severity, Severity::Warning);
        let holder: Holder = toml::from_str(r#"severity = "blocker""#).expect("word must parse");
        assert_eq!(holder.severity, Severity::Blocker);
        let bad: Result<Holder, _> = toml::from_str(r#"severity = "loud""#);
        assert!(bad.is_err());
    }
}
