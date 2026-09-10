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
        suggested_fix: "resolve the test finding".to_string(),
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
