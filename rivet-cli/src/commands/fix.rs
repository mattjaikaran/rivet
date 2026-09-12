//! `rivet fix`: auto-repair what the Verifier can deterministically repair.
//!
//! The command parses the module, runs the same Verifier as `rivet build`,
//! and deletes the declarations the rules can prove are wrong. Two finding
//! shapes are fixable by construction:
//!
//! - E2044 dead code on an unused helper or DTO: nothing references the
//!   declaration, so deleting it cannot change a route's behavior.
//! - E2046 type strictness on a foreign-decorated function, a runtime
//!   class, or a stray module-level statement: the engine has no IR home
//!   for any of them, so the generated server would drop them silently.
//!   Deleting them makes the module say what the server would run.
//!
//! Everything else stays broken on purpose. Complexity (E2042),
//! duplication (E2043), story links (E2045), and any finding that names a
//! route describe a design decision, not a mechanical slip, so the
//! command prints them with their suggested fixes and moves on.
//!
//! A deletion removes one whole top-level syntax node, so each round
//! re-parses and re-runs the Verifier: a helper that only a now-deleted
//! helper called dies on a later round. The loop converges within five
//! rounds or fails with E3013 instead of writing a module it cannot make
//! clean. Only a converged result is written back to the file.

use crate::config::RivetConfig;
use crate::diagnostic::Diagnostic;
use crate::verifier;
use crate::parser::NamedChildren;
use crate::parser::python::{
    DeclKind, Declaration, ParsedModule, parse_python_file, parse_python_module,
};
use std::path::Path;

/// Rounds the fix loop runs before giving up on a module. A chain of
/// mutually dead helpers needs one round per link, so the cap is small.
const MAX_ROUNDS: usize = 5;

/// One applied deletion, for the `Fixed ...` report line.
struct FixRecord {
    code: String,
    line: usize,
    phrase: String,
}

impl FixRecord {
    fn new(decl: &Declaration, code: &str) -> Self {
        let phrase = match (code, decl.kind) {
            ("E2044", DeclKind::Helper) => format!("removed unused helper `{}`", decl.name),
            ("E2044", DeclKind::Dto) => format!("removed unused DTO `{}`", decl.name),
            ("E2046", DeclKind::Foreign) => {
                format!("removed foreign-decorated function `{}`", decl.name)
            }
            ("E2046", DeclKind::RuntimeClass) => {
                format!("removed runtime class `{}`", decl.name)
            }
            ("E2046", DeclKind::Other) if decl.name == "statement" => {
                "removed stray module-level statement".to_string()
            }
            ("E2046", DeclKind::Other) => {
                format!("removed module-level {} statement", decl.name)
            }
            _ => format!("removed `{}`", decl.name),
        };
        Self {
            code: code.to_string(),
            line: decl.line,
            phrase,
        }
    }
}

/// Whether a finding with `code` may be auto-fixed when it targets a
/// declaration of `kind`. Findings on routes are never fixable.
fn is_fixable(code: &str, kind: DeclKind) -> bool {
    match code {
        "E2044" => matches!(kind, DeclKind::Helper | DeclKind::Dto),
        "E2046" => matches!(
            kind,
            DeclKind::Foreign | DeclKind::RuntimeClass | DeclKind::Other
        ),
        _ => false,
    }
}

/// The declaration a fixable finding targets, if it is unambiguous.
///
/// Findings and declarations line up by source line: each rule emits at
/// most one finding per declaration and always names the declaration in
/// `ast_path`. When several declarations share a line, the name
/// disambiguates; when it cannot, the finding is left for the report.
fn find_decl<'a>(module: &'a ParsedModule, diagnostic: &Diagnostic) -> Option<&'a Declaration> {
    let line = diagnostic.line?;
    let mut matches: Vec<&Declaration> = module
        .decls
        .iter()
        .filter(|decl| is_fixable(&diagnostic.error_code, decl.kind) && decl.line == line)
        .collect();
    if matches.len() > 1 {
        let path = diagnostic.ast_path.as_deref()?;
        matches.retain(|decl| {
            decl.name == path || path.strip_prefix("module.") == Some(decl.name.as_str())
        });
    }
    if matches.len() == 1 {
        Some(matches[0])
    } else {
        None
    }
}

/// The start of the whitespace-only line that ends just before `pos`, if
/// any. Module-level statements start at column 0, so a newline
/// immediately before `pos` means `pos` opens a fresh line.
fn blank_line_before(source: &[u8], pos: usize) -> Option<usize> {
    if pos == 0 || source[pos - 1] != b'\n' {
        return None;
    }
    let line_start = source[..pos - 1]
        .iter()
        .rposition(|&byte| byte == b'\n')
        .map_or(0, |index| index + 1);
    let line = &source[line_start..pos - 1];
    line.iter()
        .all(|&byte| byte == b' ' || byte == b'\t')
        .then_some(line_start)
}

/// The byte range to delete for a declaration's whole top-level node.
///
/// The range covers the node plus the newline that ends its last line,
/// widened backwards over whitespace-only lines directly above it. Blank
/// lines below the node are left alone: they separate the next surviving
/// statement. Ranges of neighboring deletions stay disjoint, so applying
/// them in descending byte order is safe.
fn removal_range(module: &ParsedModule, start_byte: usize) -> Option<(usize, usize)> {
    let root = module.tree.root_node();
    let node = root
        .named_children_all()
        .into_iter()
        .find(|child| child.start_byte() == start_byte)?;
    let source = module.source.as_bytes();
    let mut end = node.end_byte();
    if end < source.len() && source[end] == b'\n' {
        end += 1;
    }
    let mut start = node.start_byte();
    while let Some(line_start) = blank_line_before(source, start) {
        start = line_start;
    }
    Some((start, end))
}

/// Remove non-overlapping byte ranges from `source`.
///
/// Ranges are given in ascending order and applied in descending order,
/// so removing a later range never shifts the offsets of an earlier one.
fn delete_ranges(source: &str, mut ranges: Vec<(usize, usize)>) -> String {
    ranges.sort_by(|a, b| b.0.cmp(&a.0));
    let mut edited = source.to_string();
    for (start, end) in ranges {
        edited = format!("{}{}", &edited[..start], &edited[end..]);
    }
    edited
}

/// The blocker returned when five rounds cannot converge the module. The
/// leftover diagnostics ride along so an agent can see exactly what is
/// still fixable.
fn convergence_error(file: &str, diagnostics: &[Diagnostic]) -> Vec<Diagnostic> {
    let count = diagnostics.len();
    let first_line = diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.line)
        .min()
        .unwrap_or(1);
    let mut errors = vec![
        Diagnostic::blocker(
            "E3013",
            format!(
                "the fix loop did not converge within {MAX_ROUNDS} rounds; \
             {count} machine-fixable finding{} remain in {file}",
                if count == 1 { "" } else { "s" },
            ),
            "remove the listed declarations by hand and run `rivet fix` again; \
         a chain of mutually dead helpers needs one pass per link",
        )
        .located(file.to_string(), first_line),
    ];
    errors.extend(diagnostics.iter().cloned());
    errors
}

/// Parse the module, run the Verifier, delete what is machine-fixable in
/// rounds, and write the converged source back to the file.
pub fn run_fix(app_file: &Path) -> Result<(), Vec<Diagnostic>> {
    let project_dir = app_file
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| ".".into());

    let config = RivetConfig::load(&project_dir).map_err(|message| {
        vec![Diagnostic::blocker(
            "E1008",
            message,
            "repair the malformed rivet.toml, then run `rivet fix` again",
        )]
    })?;

    if !app_file.exists() {
        return Err(vec![
            Diagnostic::blocker(
                "E1008",
                format!("{} not found", app_file.display()),
                "write a `from rivet import api` module, then run `rivet fix`",
            )
            .located(project_dir.display().to_string(), 1),
        ]);
    }

    let label = app_file.display().to_string();
    let module_name = app_file
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "app".to_string());

    let mut module = parse_python_file(app_file).map_err(|diagnostic| vec![diagnostic])?;
    let mut applied: Vec<FixRecord> = Vec::new();
    let mut rounds = 0usize;

    let final_diagnostics = loop {
        let diagnostics = verifier::run_verifier(&module, &config.verifier);

        // Map every fixable finding to the whole top-level declaration it
        // names. A declaration two findings name is fixed once.
        let mut ranges: Vec<(usize, usize)> = Vec::new();
        let mut records: Vec<FixRecord> = Vec::new();
        let mut seen: Vec<usize> = Vec::new();
        for diagnostic in &diagnostics {
            let Some(decl) = find_decl(&module, diagnostic) else {
                continue;
            };
            if seen.contains(&decl.start_byte) {
                continue; // two findings naming one declaration fix it once
            }
            seen.push(decl.start_byte);
            let Some(range) = removal_range(&module, decl.start_byte) else {
                continue;
            };
            ranges.push(range);
            records.push(FixRecord::new(decl, &diagnostic.error_code));
        }

        if records.is_empty() {
            break diagnostics; // nothing fixable left: the loop converged
        }
        if rounds >= MAX_ROUNDS {
            return Err(convergence_error(&label, &diagnostics));
        }
        rounds += 1;
        applied.extend(records);

        let edited = delete_ranges(&module.source, ranges);
        module = parse_python_module(&edited, &module_name, &label)
            .map_err(|diagnostic| vec![diagnostic])?;
    };

    // Only a converged result is written; a round that aborted on a parse
    // failure already returned without touching the file.
    if !applied.is_empty() {
        std::fs::write(app_file, &module.source).map_err(|err| {
            vec![Diagnostic::blocker(
                "E1008",
                format!("failed to write {}: {err}", app_file.display()),
                "check the file permissions, then run `rivet fix` again",
            )]
        })?;
    }

    // The converged module's findings are all unfixable; report them so an
    // agent sees what stayed broken.
    for diagnostic in &final_diagnostics {
        eprintln!("{}", diagnostic.to_json());
        eprintln!("{}", diagnostic.summary());
    }

    if applied.is_empty() {
        println!("Nothing to fix in {label}: the Verifier found no machine-fixable findings.");
    }
    for record in &applied {
        println!(
            "Fixed {} {}:{}: {}",
            record.code, label, record.line, record.phrase
        );
    }
    let remaining = final_diagnostics.len();
    println!(
        "{remaining} unfixable finding{} remain.",
        if remaining == 1 { "" } else { "s" }
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::python::parse_python_file;
    use crate::test_support::ScratchDir;
    use std::fs;

    #[test]
    fn fix_removes_dead_code_and_runtime_classes_but_keeps_routes() {
        let dir = ScratchDir::new("fix-dead-code");
        let app = dir.join("app.py");
        fs::write(
            &app,
            "from rivet import api\n\
             \n\
             def unused_helper():\n    return 1\n\
             \n\
             class Widget:\n    def spin(self):\n        return 2\n\
             \n\
             @api.get(\"/ping\", stories=[\"US-1\"])\n\
             def ping() -> dict:\n    return {\"status\": \"pong\"}\n",
        )
        .expect("write app.py");

        run_fix(&app).expect("fix must succeed");

        let fixed = fs::read_to_string(&app).expect("read fixed app.py");
        assert!(
            !fixed.contains("unused_helper"),
            "dead helper must be removed"
        );
        assert!(!fixed.contains("Widget"), "runtime class must be removed");
        assert!(fixed.contains("/ping"), "the healthy route must survive");
        assert!(fixed.contains("def ping"), "the route handler must survive");

        let module = parse_python_file(&app).expect("fixed module must re-parse");
        let config = RivetConfig::default();
        let findings = verifier::run_verifier(&module, &config.verifier);
        assert!(
            findings
                .iter()
                .all(|finding| finding.error_code != "E2044" && finding.error_code != "E2046"),
            "no E2044/E2046 findings may remain after the fix: {findings:?}"
        );
    }

    #[test]
    fn healthy_module_reports_nothing_to_fix_and_writes_nothing() {
        let dir = ScratchDir::new("fix-healthy");
        let app = dir.join("app.py");
        let original = "from rivet import api\n\
             \n\
             @api.get(\"/ping\", stories=[\"US-1\"])\n\
             def ping() -> dict:\n    return {\"status\": \"pong\"}\n";
        fs::write(&app, original).expect("write app.py");

        run_fix(&app).expect("a healthy module must fix cleanly");

        let after = fs::read_to_string(&app).expect("read app.py");
        assert_eq!(after, original, "a healthy module must not be rewritten");
    }
}
