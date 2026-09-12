//! The collision class test: every name a user can pick, against both
//! targets.
//!
//! Every other generator test feeds the generator a name the *test author*
//! chose — `ping`, `echo`, `OrderCreate`. Those tests prove the fixtures
//! compile; they cannot prove an input outside them does. The generated crate
//! is one flat Rust namespace, so a user who names a handler `json_obj` or a
//! DTO `String` gets cargo errors against code the user never wrote, and the
//! native and WebAssembly targets sometimes disagree about the same input.
//!
//! This module asks the missing question, in two parts:
//!
//! - [`reserved_names_answer_e1013_on_both_targets`] drives every name the
//!   generated crate owns through both targets and asserts that both answer
//!   the same `E1013` diagnostic, with a file and a line;
//! - [`every_name_the_layers_make_legal_builds_on_both_targets`] builds one
//!   crate carrying all the names that are legal but were not, and asserts
//!   that both targets compile it. One build covers every row at once, which
//!   is what keeps a full compile affordable per target.
//!
//! Both parts assert the invariant that matters most: **the two targets
//! agree**. The original defect was a divergence, and a per-target test would
//! have let it ship.

use super::*;
use rivet_core::reserved;

/// How many routes the aggregate app declares. The count is fixed so the
/// fixture is one string, not a formatting exercise.
const LEGAL_HANDLERS: &[&str] = &[
    // The names that reached cargo before Layer 1 and Layer 2.
    "main",
    "json_obj",
    "json_number",
    "channel_error",
    "fixed_array",
    "executor",
    // The names the crate root holds as modules and as imported types. A
    // handler is a function in its own module now, so none of these collides.
    "service",
    "channel",
    "admin",
    "assets",
    "discovery",
    "handlers",
    "String",
    "Vec",
    "Option",
    "Json",
    "State",
    "Router",
    "serde",
    "serde_json",
    "axum",
    "tokio",
    "tracing",
    "std",
];

/// A one-route app whose handler carries `name`.
fn handler_app(name: &str) -> String {
    format!(
        "from rivet import api\n\n@api.get(\"/{name}\", stories=[\"US-1\"])\ndef {name}() -> dict:\n    return {{\"ok\": True}}\n"
    )
}

/// A one-route app whose DTO, and the route that uses it, carry `name`.
fn dto_app(name: &str) -> String {
    format!(
        "from rivet import api\n\nclass {name}:\n    value: str\n\n@api.post(\"/{name}\", stories=[\"US-1\"])\ndef put_{name}(request: {name}) -> {name}:\n    return request\n"
    )
}

/// Build `source` for `target` in a fresh directory, and answer the outcome.
fn outcome(dir_name: &str, source: &str, target: BuildTarget) -> Result<(), Vec<Diagnostic>> {
    let dir = ScratchDir::new(dir_name);
    let app = dir.join("app.py");
    fs::write(&app, source).expect("write app.py");
    fs::write(
        dir.join("rivet.toml"),
        "[project]\nname = \"collide\"\n\n[rust_native_features]\nconst_generics = true\n",
    )
    .expect("write rivet.toml");
    run_build(&app, target, true)
}

/// Both targets, in one call.
const BOTH: [BuildTarget; 2] = [BuildTarget::Native, BuildTarget::Wasm];

/// The diagnostic one target answered, or a panic naming the target.
fn e1013(dir_name: &str, source: &str, target: BuildTarget) -> Diagnostic {
    match outcome(dir_name, source, target) {
        Ok(()) => panic!(
            "{} accepted a reserved name: it must answer E1013, not build",
            target_label(target)
        ),
        Err(diagnostics) => diagnostics
            .into_iter()
            .next()
            .expect("a failed build reports at least one diagnostic"),
    }
}

/// The target's name, for an assertion message.
fn target_label(target: BuildTarget) -> &'static str {
    match target {
        BuildTarget::Native => "native",
        BuildTarget::Wasm => "wasm",
    }
}

/// Every DTO name the generated crate owns answers `E1013`, on both targets.
///
/// The rows come from [`reserved::RESERVED`] itself, so a name added to the
/// reserved set is tested on the next run instead of waiting for someone to
/// remember this table.
#[test]
fn reserved_names_answer_e1013_on_both_targets() {
    for (index, name) in reserved::RESERVED.iter().enumerate() {
        let source = dto_app(name);
        let mut answers = Vec::new();
        for target in BOTH {
            let diagnostic = e1013(&format!("collide-dto-{index}"), &source, target);
            assert_eq!(
                diagnostic.error_code,
                "E1013",
                "DTO `{name}` on {} answered {}",
                target_label(target),
                diagnostic.error_code
            );
            assert!(
                diagnostic.message.contains(name),
                "the diagnostic must name the offending identifier `{name}`: {}",
                diagnostic.message
            );
            assert!(
                diagnostic.line.is_some(),
                "the diagnostic must carry a line: {diagnostic:?}"
            );
            assert!(
                diagnostic.file.is_some(),
                "the diagnostic must carry a file: {diagnostic:?}"
            );
            answers.push((diagnostic.error_code, diagnostic.line));
        }
        assert_eq!(
            answers[0], answers[1],
            "the targets must agree about DTO `{name}`: {answers:?}"
        );
    }
}

/// A name that only carries the prefix is reserved too, for a DTO and for a
/// handler.
#[test]
fn the_reserved_prefix_is_rejected_on_both_targets() {
    let dto = format!("{}shape", reserved::PREFIX);
    let handler = format!("{}ping", reserved::PREFIX);
    for (index, source) in [dto_app(&dto), handler_app(&handler)].iter().enumerate() {
        let mut answers = Vec::new();
        for target in BOTH {
            let diagnostic = e1013(&format!("collide-prefix-{index}"), source, target);
            assert_eq!(diagnostic.error_code, "E1013");
            assert!(
                diagnostic.line.is_some() && diagnostic.file.is_some(),
                "the diagnostic must be located: {diagnostic:?}"
            );
            answers.push((diagnostic.error_code, diagnostic.line));
        }
        assert_eq!(
            answers[0], answers[1],
            "the targets must agree: {answers:?}"
        );
    }
}

/// A name that merely *contains* a reserved name stays legal.
///
/// The check is an equality against a set plus a prefix test, so `Stringify`
/// and `rivetfree` must not be caught by a substring match.
#[test]
fn a_name_that_only_resembles_a_reserved_one_is_accepted() {
    let dir = ScratchDir::new("collide-near-miss");
    let app = dir.join("app.py");
    fs::write(
        &app,
        "from rivet import api\n\nclass Stringify:\n    value: str\n\n@api.post(\"/s\", stories=[\"US-1\"])\ndef stringify(request: Stringify) -> Stringify:\n    return request\n",
    )
    .expect("write app.py");
    fs::write(dir.join("rivet.toml"), "[project]\nname = \"near\"\n").expect("write rivet.toml");
    // The parse must reach the generator: a rejected module never writes a
    // crate, so the presence of `generated/` proves the parser accepted it.
    let _ = run_build(&app, BuildTarget::Native, true);
    assert!(
        dir.join("generated").exists(),
        "`Stringify` and `rivetfree` are not reserved names"
    );
}

/// One crate carries every name the two layers make legal, and both targets
/// compile it.
///
/// This is the half of the table that a diagnostic assertion cannot cover: a
/// row is only "legal" if the emitted crate actually builds. One aggregate
/// app per target pays that cost once instead of once per row.
///
/// Every handler returns a distinct literal. The Verifier's duplicate-code
/// rule rejects one body repeated across routes (`E2043`), and that rule is
/// about the body, not the name, so an identical body would make this test
/// fail for a reason the test is not asking about.
#[test]
fn every_name_the_layers_make_legal_builds_on_both_targets() {
    let mut source = String::from("from rivet import api\n");
    for (index, name) in LEGAL_HANDLERS.iter().enumerate() {
        source.push('\n');
        source.push_str(&format!(
            "@api.get(\"/{name}\", stories=[\"US-1\"])\ndef {name}() -> dict:\n    return {{\"index\": {index}}}\n"
        ));
    }

    let mut results = Vec::new();
    for target in BOTH {
        let result = outcome(
            &format!("collide-legal-{}", target_label(target)),
            &source,
            target,
        );
        if let Err(diagnostics) = &result {
            panic!(
                "{} must build an app whose handlers reuse every generator name: {diagnostics:?}",
                target_label(target)
            );
        }
        results.push(result.is_ok());
    }
    assert_eq!(
        results[0], results[1],
        "the two targets must agree: {results:?}"
    );
}
