//! `rivet audit`: report the phase-1 MQI grade for one DSL module.
//!
//! The audit parses the module, runs the same Gauntlet rules as `rivet
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
//! `not_scored` with their reasons. A dimension whose rule the `[gauntlet]`
//! config disables also lands in `not_scored`.
//!
//! Story-gate findings (E2045) do not move the grade: a storyless route
//! already blocks `rivet build`, which is the gate that matters. The
//! breakdown reports them under `story_gate` so the audit cannot look clean
//! on a module that fails the build.

use crate::config::{GauntletConfig, RivetConfig};
use crate::diagnostic::{Diagnostic, Severity};
use crate::gauntlet;
use crate::parser::python::parse_python_file;
use serde_json::Value;
use std::path::Path;

/// Points a blocker finding subtracts from its dimension.
const BLOCKER_PENALTY: i64 = 20;
/// Points a warning finding subtracts from its dimension.
const WARNING_PENALTY: i64 = 10;

/// One pillar-06 dimension the phase-1 subset can score.
struct Dimension {
    /// Machine key; matches the `[gauntlet]` config key where one exists.
    key: &'static str,
    /// Pillar-06 label.
    label: &'static str,
    /// Pillar-06 weight word.
    weight: &'static str,
    /// Numeric weight: high = 3, medium = 2 (critical = 4, reserved).
    weight_n: i64,
    /// Gauntlet error code whose findings feed this dimension.
    code: &'static str,
}

/// The scored dimensions, in Gauntlet rule order.
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
    /// Fold Gauntlet findings and the config into a report.
    fn build(file: &str, findings: &[Diagnostic], config: &GauntletConfig) -> Report {
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
                reason: "rule disabled in the [gauntlet] config".into(),
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

/// Whether a dimension's Gauntlet rule is enabled by the config.
///
/// Complexity, duplicate code, and dead code have no off switch; type
/// strictness follows `strict_type_checking`.
fn dimension_enabled(dimension: &Dimension, config: &GauntletConfig) -> bool {
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

/// Parse the module, run the Gauntlet, and fold the findings into a
/// report. Findings do not fail the audit: it is a measurement, not a
/// gate. Only a config or parse failure returns diagnostics.
fn build_report(app_file: &Path) -> Result<Report, Vec<Diagnostic>> {
    let project_dir = app_file
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| ".".into());

    let config = RivetConfig::load(&project_dir)
        .map_err(|message| vec![Diagnostic::blocker("E1008", message)])?;

    if !app_file.exists() {
        return Err(vec![
            Diagnostic::blocker("E1008", format!("{} not found", app_file.display())).located(
                project_dir.display().to_string(),
                1,
                Some("write a `from rivet import api` module, then run `rivet audit`"),
            ),
        ]);
    }

    let module = parse_python_file(app_file).map_err(|diagnostic| vec![diagnostic])?;
    let findings = gauntlet::run_gauntlet(&module, &config.gauntlet);
    Ok(Report::build(&module.file, &findings, &config.gauntlet))
}

/// The MQI audit report as its JSON breakdown, for callers that want the
/// payload without printing it (for example the MCP `audit_app` tool).
pub(crate) fn audit_json(app_file: &Path) -> Result<String, Vec<Diagnostic>> {
    Ok(build_report(app_file)?.to_json())
}

/// Parse the module, run the Gauntlet, and print the MQI grade.
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
mod tests {
    use super::*;
    use crate::parser::python::parse_python_module;

    /// A clean, story-tagged module: every dimension scores 100.
    const CLEAN: &str = r#"
from rivet import api

@api.get("/ping", stories=["US-1"])
def ping() -> dict:
    return {"status": "pong"}

@api.post("/echo", stories=["US-2"])
def echo(request: dict) -> dict:
    return {"echo": request}
"#;

    /// A helper nothing calls: one dead-code warning under the defaults.
    const DEAD_CODE: &str = r#"
from rivet import api

def stale(value: int) -> int:
    return value

@api.get("/ping", stories=["US-1"])
def ping() -> dict:
    return {"status": "pong"}
"#;

    /// Two handlers that share an implementation: one duplicate blocker.
    const DUPLICATES: &str = r#"
from rivet import api

@api.get("/a", stories=["US-1"])
def a() -> dict:
    return {"status": "pong"}

@api.get("/b", stories=["US-1"])
def b() -> dict:
    return {"status": "pong"}
"#;

    /// A storyless route: the story gate fires, no dimension moves.
    const STORYLESS: &str = r#"
from rivet import api

@api.get("/ping")
def ping() -> dict:
    return {"status": "pong"}
"#;

    fn module(source: &str) -> crate::parser::python::ParsedModule {
        parse_python_module(source, "app", "app.py").expect("module must parse")
    }

    fn report_for(source: &str, config: &GauntletConfig) -> Report {
        let parsed = module(source);
        let findings = gauntlet::run_gauntlet(&parsed, config);
        Report::build(&parsed.file, &findings, config)
    }

    /// A hand-built finding, so aggregation tests do not depend on a rule.
    fn finding(code: &str, severity: Severity) -> Diagnostic {
        Diagnostic {
            error_code: code.to_string(),
            severity,
            message: "test finding".to_string(),
            file: Some("app.py".to_string()),
            line: Some(1),
            column: None,
            suggested_fix: None,
            ast_path: None,
        }
    }

    fn outcome<'a>(report: &'a Report, key: &str) -> &'a DimensionOutcome {
        report
            .scored
            .iter()
            .find(|outcome| outcome.dimension.key == key)
            .expect("dimension is scored")
    }

    #[test]
    fn grade_bands_cover_the_scale() {
        let bands = [
            (100.0, "A+"),
            (97.0, "A+"),
            (96.9, "A"),
            (93.0, "A"),
            (92.9, "A-"),
            (90.0, "A-"),
            (89.9, "B+"),
            (87.0, "B+"),
            (83.0, "B"),
            (80.0, "B-"),
            (79.9, "C+"),
            (77.0, "C+"),
            (73.0, "C"),
            (70.0, "C-"),
            (69.9, "D"),
            (60.0, "D"),
            (59.9, "F"),
            (0.0, "F"),
        ];
        for (score, grade) in bands {
            assert_eq!(grade_for(score), grade, "score {score}");
        }
    }

    #[test]
    fn clean_module_scores_a_plus_everywhere() {
        let report = report_for(CLEAN, &GauntletConfig::default());
        assert_eq!(report.overall, 100.0);
        assert_eq!(report.grade(), "A+");
        assert_eq!(report.scored.len(), 4);
        for outcome in &report.scored {
            assert_eq!(outcome.score, 100.0);
            assert_eq!((outcome.blockers, outcome.warnings), (0, 0));
        }
        assert_eq!(report.story_violations, 0);
        assert_eq!(report.not_scored.len(), 3);
    }

    #[test]
    fn blocker_findings_deduct_twenty_points() {
        // One blocker on each scored dimension: every score is 80 and the
        // weighted mean is exactly 80.
        let config = GauntletConfig::default();
        let findings = [
            finding("E2042", Severity::Blocker),
            finding("E2043", Severity::Blocker),
            finding("E2044", Severity::Blocker),
            finding("E2046", Severity::Blocker),
        ];
        let report = Report::build("app.py", &findings, &config);
        assert_eq!(report.overall, 80.0);
        assert_eq!(report.grade(), "B-");
        for outcome in &report.scored {
            assert_eq!(outcome.score, 80.0);
            assert_eq!(outcome.blockers, 1);
        }
    }

    #[test]
    fn warning_findings_deduct_ten_points_and_floor_at_zero() {
        let config = GauntletConfig::default();
        let findings = [
            finding("E2044", Severity::Warning),
            finding("E2044", Severity::Warning),
            finding("E2042", Severity::Warning),
            finding("E2042", Severity::Warning),
            finding("E2042", Severity::Warning),
            finding("E2042", Severity::Warning),
            finding("E2042", Severity::Warning),
            finding("E2042", Severity::Warning),
            finding("E2042", Severity::Warning),
            finding("E2042", Severity::Warning),
            finding("E2042", Severity::Warning),
            finding("E2042", Severity::Warning),
        ];
        let report = Report::build("app.py", &findings, &config);
        let dead = outcome(&report, "dead_code");
        assert_eq!((dead.blockers, dead.warnings), (0, 2));
        assert_eq!(dead.score, 80.0);
        // Ten warnings on complexity floor the score at zero, not below.
        let complexity = outcome(&report, "complexity");
        assert_eq!(complexity.warnings, 10);
        assert_eq!(complexity.score, 0.0);
    }

    #[test]
    fn overall_grade_weights_dimensions() {
        // One blocker on the medium duplicate dimension only: 80 carries
        // weight 2 against three 100s of weight 3, so the mean is not 95.
        let config = GauntletConfig::default();
        let findings = [finding("E2043", Severity::Blocker)];
        let report = Report::build("app.py", &findings, &config);
        assert_eq!(report.overall, 96.4);
        assert_eq!(report.grade(), "A");
        assert_eq!(outcome(&report, "duplicate_code").score, 80.0);
    }

    #[test]
    fn dead_code_warning_drops_only_that_dimension() {
        let report = report_for(DEAD_CODE, &GauntletConfig::default());
        let dead = outcome(&report, "dead_code");
        assert_eq!((dead.blockers, dead.warnings), (0, 1));
        assert_eq!(dead.score, 90.0);
        assert_eq!(report.overall, 97.3);
        assert_eq!(report.grade(), "A+");
    }

    #[test]
    fn duplicate_blocker_drops_the_redundant_code_dimension() {
        let report = report_for(DUPLICATES, &GauntletConfig::default());
        let duplicate = outcome(&report, "duplicate_code");
        assert_eq!((duplicate.blockers, duplicate.warnings), (1, 0));
        assert_eq!(duplicate.score, 80.0);
        assert_eq!(report.story_violations, 0);
    }

    #[test]
    fn story_gate_findings_do_not_move_the_grade() {
        let report = report_for(STORYLESS, &GauntletConfig::default());
        assert_eq!(report.story_violations, 1);
        assert!(report.story_enabled);
        assert_eq!(report.overall, 100.0);
        assert_eq!(report.grade(), "A+");
    }

    #[test]
    fn disabled_rule_moves_its_dimension_to_not_scored() {
        let config = GauntletConfig {
            strict_type_checking: false,
            ..GauntletConfig::default()
        };
        let report = report_for(CLEAN, &config);
        assert_eq!(report.scored.len(), 3);
        assert_eq!(report.not_scored.len(), 4);
        let disabled = report
            .not_scored
            .iter()
            .find(|entry| entry.key == "type_strictness")
            .expect("type strictness is listed");
        assert!(disabled.reason.contains("disabled"));
        // The overall mean drops the disabled dimension.
        assert_eq!(report.overall, 100.0);
    }

    #[test]
    fn json_breakdown_is_parseable_and_complete() {
        let report = report_for(DEAD_CODE, &GauntletConfig::default());
        let value: serde_json::Value =
            serde_json::from_str(&report.to_json()).expect("JSON must parse");
        assert_eq!(value["grade"], "A+");
        assert_eq!(value["score"], 97.3);
        let dead = value["dimensions"]
            .as_array()
            .expect("dimensions array")
            .iter()
            .find(|dimension| dimension["key"] == "dead_code")
            .expect("dead_code present");
        assert_eq!(dead["score"], 90.0);
        assert_eq!(dead["grade"], "A-");
        assert_eq!(dead["findings"]["warnings"], 1);
        let not_scored = value["not_scored"].as_array().expect("not_scored array");
        assert_eq!(not_scored.len(), 3);
        assert!(
            value["not_scored"][0]["reason"]
                .as_str()
                .expect("reason")
                .contains("generated-code")
        );
    }

    #[test]
    fn text_breakdown_leads_with_the_grade() {
        let report = report_for(CLEAN, &GauntletConfig::default());
        let text = report.to_text();
        assert!(text.starts_with("Grade: A+ (100.0)"));
        assert!(text.contains("Cyclomatic Complexity"));
        assert!(
            text.contains("Not scored (Rust-side): test_coverage, mutation_survival, doc_coverage")
        );
        assert!(!text.contains("Story gate"));
    }
}
