//! `rivet audit`: report the phase-1 MQI grade for one DSL module.
//!
//! The audit parses the module, runs the same Verifier rules as `rivet
//! build`, and folds the findings into the Matt Quality Index grade from
//! pillar 06 (`docs/pillars/06-matt-quality-index.md`).
//!
//! # Grade scale
//!
//! Every scored dimension starts at 100. A blocker finding subtracts 20
//! points; a warning subtracts 10. A dimension cannot fall below 0. The
//! overall score is the weighted mean of the scored dimensions (high = 3,
//! medium = 2; critical = 4 is reserved), rounded to one decimal place.
//!
//! | score | grade |
//! | --- | --- |
//! | 97.0 and above | A+ |
//! | 93.0 to 96.9 | A |
//! | 90.0 to 92.9 | A- |
//! | 87.0 to 89.9 | B+ |
//! | 83.0 to 86.9 | B |
//! | 80.0 to 82.9 | B- |
//! | 77.0 to 79.9 | C+ |
//! | 73.0 to 76.9 | C |
//! | 70.0 to 72.9 | C- |
//! | 60.0 to 69.9 | D |
//! | below 60.0 | F |
//!
//! # Phase-1 dimensions
//!
//! | key | pillar-06 label | weight | finding code |
//! | --- | --- | --- | --- |
//! | `complexity` | Cyclomatic Complexity | high | E2042 |
//! | `duplicate_code` | Redundant Code | medium | E2043 |
//! | `dead_code` | Dead Code | high | E2044 |
//! | `type_strictness` | Type Strictness | high | E2046 |
//!
//! Test coverage, mutation survival, and documentation coverage measure
//! the generated Rust crate, so they stay Rust-side until the
//! generated-code test story exists. The breakdown lists them under
//! `not_scored` with their reasons. A dimension whose rule the `[verifier]`
//! config disables also lands in `not_scored`.
//!
//! Story-gate findings (E2045) do not move the grade: a storyless route
//! already blocks `rivet build`, which is the gate that matters. The
//! breakdown reports them under `story_gate` so the audit cannot look clean
//! on a module that fails the build.

use crate::config::{RivetConfig, VerifierConfig};
use crate::diagnostic::{Diagnostic, Severity};
use crate::parser::python::parse_python_file;
use crate::verifier;
use serde_json::Value;
use std::path::Path;

/// Points a blocker finding subtracts from its dimension.
const BLOCKER_PENALTY: i64 = 20;
/// Points a warning finding subtracts from its dimension.
const WARNING_PENALTY: i64 = 10;

/// One pillar-06 dimension the phase-1 subset can score.
struct Dimension {
    /// Machine key; matches the `[verifier]` config key where one exists.
    key: &'static str,
    /// Pillar-06 label.
    label: &'static str,
    /// Pillar-06 weight word.
    weight: &'static str,
    /// Numeric weight: high = 3, medium = 2 (critical = 4, reserved).
    weight_n: i64,
    /// Verifier error code whose findings feed this dimension.
    code: &'static str,
}

/// The scored dimensions, in Verifier rule order.
static DIMENSIONS: [Dimension; 4] = [
    Dimension {
        key: "complexity",
        label: "Cyclomatic Complexity",
        weight: "high",
        weight_n: 3,
        code: "E2042",
    },
    Dimension {
        key: "duplicate_code",
        label: "Redundant Code",
        weight: "medium",
        weight_n: 2,
        code: "E2043",
    },
    Dimension {
        key: "dead_code",
        label: "Dead Code",
        weight: "high",
        weight_n: 3,
        code: "E2044",
    },
    Dimension {
        key: "type_strictness",
        label: "Type Strictness",
        weight: "high",
        weight_n: 3,
        code: "E2046",
    },
];

/// A dimension the metric cannot measure yet, with the reason.
struct NotScored {
    key: &'static str,
    label: &'static str,
    weight: &'static str,
    reason: String,
}

/// Dimensions whose metric needs generated code or generated-code tests.
const RUST_SIDE: [(&str, &str, &str, &str); 3] = [
    (
        "test_coverage",
        "Test Coverage",
        "high",
        "counts tests of the generated Rust crate",
    ),
    (
        "mutation_survival",
        "Mutation Survival",
        "critical",
        "requires the generated-code test story (cargo-mutants)",
    ),
    (
        "doc_coverage",
        "Documentation Coverage",
        "medium",
        "counts public functions of the generated Rust crate",
    ),
];

/// The measured result for one dimension.
struct DimensionOutcome {
    dimension: &'static Dimension,
    blockers: usize,
    warnings: usize,
    score: f64,
}

impl DimensionOutcome {
    fn grade(&self) -> &'static str {
        grade_for(self.score)
    }
}

/// The full audit result: scored dimensions, exclusions, and the grade.
struct Report {
    file: String,
    scored: Vec<DimensionOutcome>,
    not_scored: Vec<NotScored>,
    story_enabled: bool,
    story_violations: usize,
    overall: f64,
}

impl Report {
    /// Fold Verifier findings and the config into a report.
    fn build(file: &str, findings: &[Diagnostic], config: &VerifierConfig) -> Report {
        let scored: Vec<DimensionOutcome> = DIMENSIONS
            .iter()
            .filter(|dimension| dimension_enabled(dimension, config))
            .map(|dimension| score_dimension(dimension, findings))
            .collect();

        let mut not_scored: Vec<NotScored> = RUST_SIDE
            .iter()
            .map(|(key, label, weight, reason)| NotScored {
                key,
                label,
                weight,
                reason: format!("{reason}; generated-code tests land in a later phase"),
            })
            .collect();
        for dimension in DIMENSIONS.iter().filter(|d| !dimension_enabled(d, config)) {
            not_scored.push(NotScored {
                key: dimension.key,
                label: dimension.label,
                weight: dimension.weight,
                reason: "rule disabled in the [verifier] config".into(),
            });
        }

        let story_violations = findings
            .iter()
            .filter(|finding| finding.error_code == "E2045")
            .count();

        Report {
            file: file.to_string(),
            overall: overall_score(&scored),
            scored,
            not_scored,
            story_enabled: config.stories_required,
            story_violations,
        }
    }

    fn grade(&self) -> &'static str {
        grade_for(self.overall)
    }

    /// The JSON breakdown an agent can parse.
    ///
    /// Built from [`serde_json::Value`] constructors only (like
    /// [`crate::diagnostic::Diagnostic::to_json`]) so it cannot panic: the
    /// `json!` macro expands to `unwrap` calls, which clippy bans.
    fn to_json(&self) -> String {
        let dimensions: Vec<serde_json::Value> = self
            .scored
            .iter()
            .map(|outcome| {
                let d = outcome.dimension;
                let mut findings = serde_json::Map::new();
                findings.insert("blockers".into(), Value::from(outcome.blockers));
                findings.insert("warnings".into(), Value::from(outcome.warnings));
                let mut object = serde_json::Map::new();
                object.insert("key".into(), Value::String(d.key.into()));
                object.insert("label".into(), Value::String(d.label.into()));
                object.insert("weight".into(), Value::String(d.weight.into()));
                object.insert("score".into(), Value::from(outcome.score));
                object.insert("grade".into(), Value::String(outcome.grade().into()));
                object.insert("findings".into(), Value::Object(findings));
                Value::Object(object)
            })
            .collect();
        let not_scored: Vec<serde_json::Value> = self
            .not_scored
            .iter()
            .map(|entry| {
                let mut object = serde_json::Map::new();
                object.insert("key".into(), Value::String(entry.key.into()));
                object.insert("label".into(), Value::String(entry.label.into()));
                object.insert("weight".into(), Value::String(entry.weight.into()));
                object.insert("reason".into(), Value::String(entry.reason.clone()));
                Value::Object(object)
            })
            .collect();
        let mut gate = serde_json::Map::new();
        gate.insert("enabled".into(), Value::from(self.story_enabled));
        gate.insert("violations".into(), Value::from(self.story_violations));
        let mut object = serde_json::Map::new();
        object.insert("tool".into(), Value::String("rivet-audit".into()));
        object.insert("file".into(), Value::String(self.file.clone()));
        object.insert("grade".into(), Value::String(self.grade().into()));
        object.insert("score".into(), Value::from(self.overall));
        object.insert("dimensions".into(), Value::Array(dimensions));
        object.insert("not_scored".into(), Value::Array(not_scored));
        object.insert("story_gate".into(), Value::Object(gate));
        Value::Object(object).to_string()
    }

    /// The human-readable summary.
    fn to_text(&self) -> String {
        let mut lines = vec![format!("Grade: {} ({:.1})\n", self.grade(), self.overall)];
        lines.push(format!(
            "{:<24} {:<7} {:>6} {:>5}  {}",
            "Dimension", "Weight", "Score", "Grade", "Findings"
        ));
        for outcome in &self.scored {
            let d = outcome.dimension;
            lines.push(format!(
                "{:<24} {:<7} {:>6.1} {:>5}  {}",
                d.label,
                d.weight,
                outcome.score,
                outcome.grade(),
                findings_summary(outcome.blockers, outcome.warnings)
            ));
        }
        if !self.not_scored.is_empty() {
            let keys = self
                .not_scored
                .iter()
                .map(|entry| entry.key)
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("\nNot scored (Rust-side): {keys}"));
        }
        if self.story_enabled && self.story_violations > 0 {
            lines.push(format!(
                "\nStory gate: {} violation(s); the module fails `rivet build`.",
                self.story_violations
            ));
        }
        lines.join("\n")
    }
}

/// Whether a dimension's Verifier rule is enabled by the config.
///
/// Complexity, duplicate code, and dead code have no off switch; type
/// strictness follows `strict_type_checking`.
fn dimension_enabled(dimension: &Dimension, config: &VerifierConfig) -> bool {
    match dimension.key {
        "type_strictness" => config.strict_type_checking,
        _ => true,
    }
}

/// Measure one dimension over the findings that name its error code.
fn score_dimension(dimension: &'static Dimension, findings: &[Diagnostic]) -> DimensionOutcome {
    let blockers = findings
        .iter()
        .filter(|finding| {
            finding.error_code == dimension.code && finding.severity == Severity::Blocker
        })
        .count();
    let warnings = findings
        .iter()
        .filter(|finding| {
            finding.error_code == dimension.code && finding.severity == Severity::Warning
        })
        .count();
    let penalty = blockers as i64 * BLOCKER_PENALTY + warnings as i64 * WARNING_PENALTY;
    let score = (100 - penalty).max(0) as f64;
    DimensionOutcome {
        dimension,
        blockers,
        warnings,
        score,
    }
}

/// The weighted mean of the scored dimensions, rounded to one decimal.
fn overall_score(scored: &[DimensionOutcome]) -> f64 {
    if scored.is_empty() {
        return 0.0;
    }
    let weighted: f64 = scored
        .iter()
        .map(|outcome| outcome.score * outcome.dimension.weight_n as f64)
        .sum();
    let total_weight: f64 = scored
        .iter()
        .map(|outcome| outcome.dimension.weight_n as f64)
        .sum();
    ((weighted / total_weight) * 10.0).round() / 10.0
}

/// Map a numeric score to a letter grade on the A+ to F scale.
fn grade_for(score: f64) -> &'static str {
    match score {
        s if s >= 97.0 => "A+",
        s if s >= 93.0 => "A",
        s if s >= 90.0 => "A-",
        s if s >= 87.0 => "B+",
        s if s >= 83.0 => "B",
        s if s >= 80.0 => "B-",
        s if s >= 77.0 => "C+",
        s if s >= 73.0 => "C",
        s if s >= 70.0 => "C-",
        s if s >= 60.0 => "D",
        _ => "F",
    }
}

/// `2 blockers`, `1 warning`, or `-` when a dimension is clean.
fn findings_summary(blockers: usize, warnings: usize) -> String {
    match (blockers, warnings) {
        (0, 0) => "-".to_string(),
        (b, 0) => format!("{b} blocker{}", plural(b)),
        (0, w) => format!("{w} warning{}", plural(w)),
        (b, w) => format!("{b} blocker{}, {w} warning{}", plural(b), plural(w)),
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

/// Parse the module, run the Verifier, and fold the findings into a
/// report. Findings do not fail the audit: it is a measurement, not a
/// gate. Only a config or parse failure returns diagnostics.
fn build_report(app_file: &Path) -> Result<Report, Vec<Diagnostic>> {
    let project_dir = app_file
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| ".".into());

    let config = RivetConfig::load(&project_dir).map_err(|message| {
        vec![Diagnostic::blocker(
            "E1008",
            message,
            "correct the invalid `rivet.toml` value the message names (or fix the file's permissions), then rerun the command",
        )]
    })?;

    if !app_file.exists() {
        return Err(vec![
            Diagnostic::blocker(
                "E1008",
                format!("{} not found", app_file.display()),
                "write a `from rivet import api` module, then run `rivet audit`",
            )
            .located(project_dir.display().to_string(), 1),
        ]);
    }

    let module = parse_python_file(app_file).map_err(|diagnostic| vec![diagnostic])?;
    let findings = verifier::run_verifier(&module, &config.verifier);
    Ok(Report::build(&module.file, &findings, &config.verifier))
}

/// The MQI audit report as its JSON breakdown, for callers that want the
/// payload without printing it (for example the MCP `audit_app` tool).
pub(crate) fn audit_json(app_file: &Path) -> Result<String, Vec<Diagnostic>> {
    Ok(build_report(app_file)?.to_json())
}

/// Parse the module, run the Verifier, and print the MQI grade.
pub fn run_audit(app_file: &Path, json: bool) -> Result<(), Vec<Diagnostic>> {
    let report = build_report(app_file)?;
    if json {
        println!("{}", report.to_json());
    } else {
        println!("{}", report.to_text());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
