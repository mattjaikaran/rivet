//! Check that Verifier error-code documentation stays coherent: the E-code
//! table in `rivet-cli/src/verifier/mod.rs`, its `pub mod` declarations, and
//! the rule-module files on disk must agree with each module's doc codes.
//!
//! Usage: `cargo run -p constraint-tools --bin check-rule-modules [root]`.

use std::collections::{BTreeMap, BTreeSet};
use std::{fs, path::Path, process::ExitCode};

fn main() -> ExitCode {
    let root_arg = std::env::args().nth(1).unwrap_or_else(|| ".".to_string());
    let root = Path::new(&root_arg);
    let dir = root.join("rivet-cli").join("src").join("verifier");
    let mod_abs = dir.join("mod.rs");
    let mod_path = rel_path(root, &mod_abs);
    let mod_text = match fs::read_to_string(&mod_abs) {
        Ok(text) => text,
        Err(err) => return die(format!("error: cannot read {mod_path}: {err}")),
    };
    let mut files: Vec<(String, String)> = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let abs = entry.path();
            let name = entry.file_name();
            let keep = abs.is_file()
                && name
                    .to_str()
                    .is_some_and(|n| n != "mod.rs" && n.ends_with(".rs"));
            if keep && let Ok(text) = fs::read_to_string(&abs) {
                files.push((rel_path(root, &abs), text));
            }
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let out = check_verifier(&mod_path, &mod_text, &files);
    if out.is_empty() {
        let rows = table_rows(&mod_text);
        let mut lo = rows[0].1.as_str();
        let mut hi = lo;
        for (_, code, _) in &rows {
            lo = lo.min(code.as_str());
            hi = hi.max(code.as_str());
        }
        println!(
            "pass: {} rule modules, codes {lo}..{hi} in sync",
            rows.len()
        );
        ExitCode::SUCCESS
    } else {
        for line in &out {
            println!("{line}");
        }
        println!("fail: {} violation(s) found", out.len());
        ExitCode::FAILURE
    }
}

fn die(err: String) -> ExitCode {
    eprintln!("{err}");
    ExitCode::FAILURE
}

fn rel_path(root: &Path, abs: &Path) -> String {
    abs.strip_prefix(root)
        .unwrap_or(abs)
        .to_string_lossy()
        .replace('\\', "/")
}

fn module_stem(path: &str) -> &str {
    Path::new(path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(path)
}

/// One `(line, code, module)` per `| E.... | [\`module\`] |` row in the
/// leading `//!` doc block of the harness.
fn table_rows(mod_text: &str) -> Vec<(usize, String, String)> {
    let mut rows = Vec::new();
    for (idx, raw) in mod_text.lines().enumerate() {
        let line = raw.trim_start();
        if !line.starts_with("//!") {
            break;
        }
        let content = line[3..].trim();
        if content.starts_with("| E")
            && let Some((code, module)) = parse_row(content)
        {
            rows.push((idx + 1, code, module));
        }
    }
    rows
}

fn parse_row(content: &str) -> Option<(String, String)> {
    let cells: Vec<&str> = content.split('|').collect();
    let code = cells.get(1)?.trim();
    if !is_code(code) {
        return None;
    }
    let module = cells.get(2)?.trim().split_once('`')?.1.split_once('`')?.0;
    if module.is_empty() {
        return None;
    }
    Some((code.to_string(), module.to_string()))
}

fn is_code(code: &str) -> bool {
    code.len() == 5 && code.starts_with('E') && code[1..].chars().all(|c| c.is_ascii_digit())
}

/// Module names declared with `pub mod <name>;` anywhere in the file.
fn pub_mods(mod_text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in mod_text.lines() {
        let name = raw
            .trim()
            .strip_prefix("pub mod ")
            .and_then(|rest| rest.strip_suffix(';'))
            .unwrap_or("");
        if !name.is_empty() {
            out.push(name.to_string());
        }
    }
    out
}

fn doc_block(text: &str) -> String {
    let mut out = String::new();
    for raw in text.lines() {
        let line = raw.trim_start();
        if !line.starts_with("//!") {
            break;
        }
        out.push_str(&line[3..]);
        out.push('\n');
    }
    out
}

/// Unique sorted E-codes in `text`: `E` plus four ASCII digits, delimited on
/// both sides by non-alphanumeric characters or the text boundary.
fn e_codes(text: &str) -> Vec<String> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let clean = chars[i].1 == 'E'
            && (i == 0 || !chars[i - 1].1.is_alphanumeric())
            && i + 5 <= chars.len()
            && chars[i + 1..i + 5].iter().all(|(_, c)| c.is_ascii_digit())
            && (i + 5 == chars.len() || !chars[i + 5].1.is_alphanumeric());
        if clean {
            out.push(chars[i..i + 5].iter().map(|(_, c)| *c).collect());
            i += 5;
        } else {
            i += 1;
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Cross-check table, declarations, files, and doc codes; returns one
/// `path:line: message` per violation, sorted for stable output.
fn check_verifier(mod_path: &str, mod_text: &str, files: &[(String, String)]) -> Vec<String> {
    let rows = table_rows(mod_text);
    let declared = pub_mods(mod_text);
    let mut out = Vec::new();
    if rows.is_empty() || files.is_empty() {
        out.push(format!(
            "{mod_path}:1: error-code table is empty or no rule modules found"
        ));
        return out;
    }
    let mut code_line: BTreeMap<&str, usize> = BTreeMap::new();
    for (line, code, _) in &rows {
        let first = code_line.entry(code.as_str()).or_insert(*line);
        if *first != *line {
            out.push(format!(
                "{mod_path}:{line}: duplicate table row: code {code} already documented at line {first}"
            ));
        }
    }
    let mut module_row: BTreeMap<&str, (&str, usize)> = BTreeMap::new();
    for (line, code, module) in &rows {
        module_row
            .entry(module.as_str())
            .or_insert((code.as_str(), *line));
    }
    let stems: BTreeSet<&str> = files.iter().map(|(path, _)| module_stem(path)).collect();
    for (module, &(code, line)) in &module_row {
        if !stems.contains(module) {
            out.push(format!(
                "{mod_path}:{line}: code {code} names module {module}, but no {module}.rs exists"
            ));
            continue;
        }
        if !declared.iter().any(|name| name == module) {
            out.push(format!(
                "{mod_path}:{line}: module {module} is documented and has a rule file, but no \
                 `pub mod {module};` declaration"
            ));
        }
    }
    for (path, text) in files {
        let module = module_stem(path);
        let Some(&(code, _)) = module_row.get(module) else {
            out.push(format!(
                "{path}:1: module {module} has a rule file but no row in the error-code table"
            ));
            continue;
        };
        if !e_codes(&doc_block(text)).iter().any(|c| c == code) {
            out.push(format!("{path}:1: doc comment does not document {code}"));
        }
        let codes = e_codes(text);
        if !codes.iter().any(|c| c == code) {
            out.push(format!(
                "{path}:1: file E-codes are missing the table code {code}"
            ));
        }
        for extra in &codes {
            if extra != code {
                out.push(format!(
                    "{path}:1: file documents extra E-code {extra}; its table code is {code}"
                ));
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    const MOD: &str = "rivet-cli/src/verifier/mod.rs";
    const DIR: &str = "rivet-cli/src/verifier/";
    const HEAD: &str = "//! Head.\n//!\n//! | code | rule | meaning |\n//! | --- | --- | --- |\n";

    fn table(rows: &[(&str, &str)], decls: &[&str]) -> String {
        let mut text = String::from(HEAD);
        for (code, module) in rows {
            text.push_str(&format!("//! | {code} | [`{module}`] | meaning |\n"));
        }
        for module in decls {
            text.push_str(&format!("pub mod {module};\n"));
        }
        text
    }

    fn run(rows: &[(&str, &str)], decls: &[&str], mods: &[(&str, &str)]) -> Vec<String> {
        let files: Vec<(String, String)> = mods
            .iter()
            .map(|(module, code)| {
                (
                    format!("{DIR}{module}.rs"),
                    format!("//! Rule (`{code}`): x.\n\nfn h() {{}}\n"),
                )
            })
            .collect();
        run_text(&table(rows, decls), &files)
    }

    fn run_text(table: &str, files: &[(String, String)]) -> Vec<String> {
        check_verifier(MOD, table, files)
    }

    #[test]
    fn parsers_find_rows_codes_and_decls() {
        let rows = table(&[("E2042", "complexity"), ("E2046", "type_strict")], &[]);
        let parsed = table_rows(&rows);
        assert_eq!(
            parsed[0],
            (5, "E2042".to_string(), "complexity".to_string())
        );
        assert_eq!(
            parsed[1],
            (6, "E2046".to_string(), "type_strict".to_string())
        );
        let text = "//! (`E2042`) and \"E2046\" match; E20421 and xE2042 do not.";
        assert_eq!(e_codes(text), ["E2042", "E2046"]);
        let mods = "pub mod a;\nmod b;\npub(crate) mod c;\npub mod d;\n";
        assert_eq!(pub_mods(mods), ["a", "d"]);
    }

    #[test]
    fn coherent_verifier_passes() {
        let out = run(
            &[("E2042", "complexity"), ("E2043", "duplicate")],
            &["complexity", "duplicate"],
            &[("complexity", "E2042"), ("duplicate", "E2043")],
        );
        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn drift_kinds_are_reported() {
        let cases: &[(&[(&str, &str)], &[&str], &[(&str, &str)], &str)] = &[
            (
                &[("E2042", "complexity"), ("E2042", "duplicate")],
                &["complexity", "duplicate"],
                &[("complexity", "E2042"), ("duplicate", "E2042")],
                "duplicate table row",
            ),
            (
                &[("E2042", "complexity"), ("E2099", "ghost")],
                &["complexity", "ghost"],
                &[("complexity", "E2042")],
                "ghost.rs",
            ),
            (
                &[("E2042", "complexity")],
                &["complexity", "story_link"],
                &[("complexity", "E2042"), ("story_link", "E2045")],
                "no row in the error-code table",
            ),
            (
                &[("E2042", "complexity")],
                &[],
                &[("complexity", "E2042")],
                "no `pub mod complexity;` declaration",
            ),
        ];
        for (rows, decls, mods, needle) in cases {
            let out = run(rows, decls, mods);
            assert_eq!(out.len(), 1, "{out:?}");
            assert!(out[0].contains(needle), "{out:?}");
        }
        let empty = run(&[], &[], &[("complexity", "E2042")]);
        assert_eq!(empty.len(), 1, "{empty:?}");
        assert!(empty[0].contains("empty or no rule modules"), "{empty:?}");
    }

    #[test]
    fn file_doc_issues_are_reported() {
        let text = table(&[("E2042", "complexity")], &["complexity"]);
        let body = "//! Rule prose.\n\nfn helper() { let _ = \"E2042\"; }\n";
        let out = run_text(&text, &[file("complexity", body)]);
        assert_eq!(out.len(), 1, "{out:?}");
        assert!(
            out[0].contains("doc comment does not document E2042"),
            "{out:?}"
        );
        let body = "//! Rule (`E2042`): x.\n\nfn helper() { let _ = \"E2099\"; }\n";
        let out = run_text(&text, &[file("complexity", body)]);
        assert_eq!(out.len(), 1, "{out:?}");
        assert!(out[0].contains("extra E-code E2099"), "{out:?}");
        let out = run_text(
            &text,
            &[file("complexity", "//! Rule prose.\n\nfn helper() {}\n")],
        );
        assert_eq!(out.len(), 2, "{out:?}");
        assert!(
            out.iter()
                .any(|m| m.contains("doc comment does not document E2042"))
        );
        assert!(
            out.iter()
                .any(|m| m.contains("missing the table code E2042"))
        );
    }

    fn file(module: &str, body: &str) -> (String, String) {
        (format!("{DIR}{module}.rs"), body.to_string())
    }
}
