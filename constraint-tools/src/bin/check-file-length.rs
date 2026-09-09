//! File-ceiling constraint checker for Rust sources in this workspace.
//!
//! Every `.rs` file under a workspace member source tree must stay within a
//! documented line ceiling. This binary walks `constraint-tools/src`,
//! `rivet-cli/src`, and `rivet-core/src` below an optional root directory,
//! counts the lines of each `.rs` file (skipping any `target` directory),
//! and reports every file above its ceiling.
//!
//! Ceiling policy, resolved in this order:
//!
//! 1. An exact relative path in [`GRANDFATHERED`] uses that ceiling. Those
//!    legacy files are frozen ratchets at today's measured sizes: they may
//!    not grow until split below the default ceiling.
//! 2. A Gauntlet rule module, any file under `rivet-cli/src/gauntlet/`
//!    except the `mod.rs` harness, uses [`GAUNTLET_CEILING`]. Rule modules
//!    include their inline tests, so 300 lines is tight but generous.
//! 3. Every other file uses [`DEFAULT_CEILING`].
//!
//! A violation prints as `path:count: message`, sorted by path, followed by
//! the summary `fail: N violation(s) found`. A clean tree prints one line:
//! `pass: N Rust files under ceiling`.
//!
//! Usage: `cargo run -p constraint-tools --bin check-file-length [root]`.

use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

/// Ceiling for any file without a more specific policy.
const DEFAULT_CEILING: usize = 400;

/// Ceiling for one Gauntlet rule module (inline tests included).
const GAUNTLET_CEILING: usize = 300;

/// Path prefix that marks the Gauntlet rule directory.
const GAUNTLET_PREFIX: &str = "rivet-cli/src/gauntlet/";

/// Member source trees that this checker scans below the root.
const MEMBER_SRC_TREES: &[&str] = &["constraint-tools/src", "rivet-cli/src", "rivet-core/src"];

/// Legacy files frozen at today's measured line counts. Each entry is a
/// ratchet: the file may not grow until it is split below the default.
const GRANDFATHERED: &[(&str, usize)] = &[
    ("rivet-cli/src/parser/python.rs", 728),
    ("rivet-cli/src/transpiler/rust.rs", 699),
    ("rivet-cli/src/commands/audit.rs", 679),
    ("rivet-cli/src/parser/validate.rs", 444),
];

/// Resolve the line ceiling for a relative source path.
///
/// Exact grandfather match wins, then the Gauntlet rule-module rule, then
/// the default. `rivet-cli/src/gauntlet/mod.rs` is the harness, not a rule
/// module, so it falls through to the default.
fn ceiling_for(rel: &str) -> usize {
    if let Some((_, ceiling)) = GRANDFATHERED.iter().find(|(path, _)| *path == rel) {
        return *ceiling;
    }
    let rest = match rel.strip_prefix(GAUNTLET_PREFIX) {
        Some(rest) => rest,
        None => return DEFAULT_CEILING,
    };
    let is_harness = rest.rsplit('/').next() == Some("mod.rs");
    if is_harness {
        DEFAULT_CEILING
    } else {
        GAUNTLET_CEILING
    }
}

/// The violation line for a measured file, when its count exceeds its
/// ceiling. Strictly greater than the ceiling is required to fail.
fn violation_for(rel: &str, count: usize) -> Option<String> {
    let ceiling = ceiling_for(rel);
    if count > ceiling {
        Some(format!("{rel}:{count}: exceeds ceiling of {ceiling} lines"))
    } else {
        None
    }
}

/// Recursively measure `.rs` files under `root/rel`.
///
/// Measured files collect into `files`; unreadable files push their
/// violation line into `violations`. Directories named `target` are
/// skipped, as are member trees that do not exist below the root.
fn walk(root: &Path, rel: &str, files: &mut Vec<(String, usize)>, violations: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(root.join(rel)) else {
        return;
    };
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        if entry.file_name() == "target" {
            continue;
        }
        let rel_child = format!("{rel}/{}", entry.file_name().to_string_lossy());
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            walk(root, &rel_child, files, violations);
        } else if file_type.is_file() && rel_child.ends_with(".rs") {
            match fs::read_to_string(root.join(&rel_child)) {
                Ok(content) => files.push((rel_child, content.lines().count())),
                Err(_) => violations.push(format!("{rel_child}:1: unreadable file")),
            }
        }
    }
}

/// Measure every `.rs` file in the member trees below `root`.
///
/// Returns the number of files measured and the violation lines sorted by
/// path. Pure decision logic stays in [`ceiling_for`] and
/// [`violation_for`]; this function only walks the filesystem.
fn check_root(root: &Path) -> (usize, Vec<String>) {
    let mut files: Vec<(String, usize)> = Vec::new();
    let mut violations: Vec<String> = Vec::new();
    for member in MEMBER_SRC_TREES {
        walk(root, member, &mut files, &mut violations);
    }
    violations.extend(
        files
            .iter()
            .filter_map(|(rel, count)| violation_for(rel, *count)),
    );
    violations.sort();
    (files.len(), violations)
}

fn main() -> ExitCode {
    let root = env::args().nth(1).unwrap_or_else(|| ".".to_string());
    let (checked, violations) = check_root(Path::new(&root));
    if violations.is_empty() {
        println!("pass: {checked} Rust files under ceiling");
        ExitCode::SUCCESS
    } else {
        for violation in &violations {
            println!("{violation}");
        }
        println!("fail: {} violation(s) found", violations.len());
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grandfather_exact_path_uses_its_ceiling() {
        assert_eq!(ceiling_for("rivet-cli/src/parser/python.rs"), 728);
    }

    #[test]
    fn gauntlet_rule_modules_get_300() {
        assert_eq!(ceiling_for("rivet-cli/src/gauntlet/complexity.rs"), 300);
    }

    #[test]
    fn gauntlet_mod_rs_is_the_harness_not_a_rule() {
        assert_eq!(ceiling_for("rivet-cli/src/gauntlet/mod.rs"), 400);
    }

    #[test]
    fn unmatched_paths_fall_through_to_default() {
        assert_eq!(ceiling_for("rivet-cli/src/parser/lexer.rs"), 400);
        assert_eq!(ceiling_for("rivet-core/src/lib.rs"), 400);
    }

    #[test]
    fn boundary_is_strictly_greater() {
        assert_eq!(
            violation_for("rivet-cli/src/gauntlet/duplicate.rs", 300),
            None
        );
        assert_eq!(
            violation_for("rivet-cli/src/gauntlet/duplicate.rs", 301),
            Some(
                "rivet-cli/src/gauntlet/duplicate.rs:301: exceeds ceiling of 300 lines".to_string()
            )
        );
    }

    #[test]
    fn scan_reports_only_oversized_files() -> std::io::Result<()> {
        let root = std::env::temp_dir().join(format!("check-file-length-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        write_lines(&root, "rivet-cli/src/parser/python.rs", 728)?;
        write_lines(&root, "rivet-cli/src/gauntlet/complexity.rs", 301)?;
        write_lines(&root, "rivet-core/src/lib.rs", 399)?;

        let (checked, violations) = check_root(&root);
        fs::remove_dir_all(&root)?;

        assert_eq!(checked, 3);
        assert_eq!(
            violations,
            vec!["rivet-cli/src/gauntlet/complexity.rs:301: exceeds ceiling of 300 lines"]
        );
        Ok(())
    }

    /// Write `count` newline-terminated comment lines at `root/rel`.
    fn write_lines(root: &Path, rel: &str, count: usize) -> std::io::Result<()> {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut content = String::with_capacity(count * 9);
        for _ in 0..count {
            content.push_str("// filler\n");
        }
        fs::write(path, content)
    }
}
