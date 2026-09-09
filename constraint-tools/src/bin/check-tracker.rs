//! Check that the task tracker stays coherent with the documented workflow.
//!
//! The workflow in `tasks/todo.md` requires a finished item to be ticked,
//! dated, given a commit, and moved to `tasks/completed.md` in the same
//! change. This check reads both files and reports drift: malformed checkbox
//! markers, finished items left in the todo list, duplicate items, items that
//! also appear in the completed list, and checkboxes parked under a section
//! marked complete.
//!
//! Usage: `cargo run -p constraint-tools --bin check-tracker [root]`.
//! The optional `root` argument defaults to `.`. On success print
//! `pass: tracker coherent (N todo items, M completed items)` and exit 0.
//! Otherwise print each `path:line: message` violation and exit 1.

use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

const TODO_PATH: &str = "tasks/todo.md";
const COMPLETED_PATH: &str = "tasks/completed.md";

const MSG_MISSING: &str = "missing or unreadable";
const MSG_MARKER: &str = "malformed checkbox marker";
const MSG_STALE: &str = "finished item not moved to completed.md";
const MSG_DUP: &str = "duplicate item";
const MSG_CROSS: &str = "item also present in completed.md";
const MSG_SECTION: &str = "open item under completed section";

/// A violation report: file path relative to the root, 1-based line, message.
type Report = (&'static str, usize, &'static str);

/// Parse a checkbox bullet into its marker kind and trimmed body. `kind` is
/// the character between the brackets: a space means open, `x` means done,
/// `~` means in progress. Return `None` for a bullet that opens `- [` but
/// whose marker is malformed, for example `- [q]` or `- [x]glued`.
fn parse_checkbox(line: &str) -> Option<(char, &str)> {
    let t = line.trim_start();
    const FORMS: [(char, &str); 3] = [(' ', "- [ ]"), ('x', "- [x]"), ('~', "- [~]")];
    for (kind, marker) in FORMS {
        let Some(rest) = t.strip_prefix(marker) else {
            continue;
        };
        return match rest.strip_prefix(' ') {
            Some(body) => Some((kind, body.trim())),
            None if rest.is_empty() => Some((kind, "")),
            None => None,
        };
    }
    None
}

/// Apply the todo.md rules. Return the reports and the healthy open or
/// in-progress items as (line, trimmed body) pairs for the duplicate checks.
fn scan_todo(todo: &str) -> (Vec<Report>, Vec<(usize, &str)>) {
    let mut out = Vec::new();
    let mut open = Vec::new();
    let mut complete_section = false;
    for (idx, line) in todo.lines().enumerate() {
        let n = idx + 1;
        let t = line.trim_start();
        if t.starts_with("## ") {
            complete_section = t.contains("(complete");
            continue;
        }
        if !t.starts_with("- [") {
            continue;
        }
        match parse_checkbox(line) {
            None => out.push((TODO_PATH, n, MSG_MARKER)),
            Some(('x', _)) => out.push((TODO_PATH, n, MSG_STALE)),
            Some(_) if complete_section => out.push((TODO_PATH, n, MSG_SECTION)),
            Some((_, body)) => open.push((n, body)),
        }
    }
    (out, open)
}

/// Collect completed.md bullets as (line, trimmed body) pairs. A bullet is
/// any line whose trimmed form starts with `- `.
fn scan_completed(completed: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    for (idx, line) in completed.lines().enumerate() {
        let t = line.trim_start();
        if let Some(body) = t.strip_prefix("- ") {
            out.push((idx + 1, body.trim()));
        }
    }
    out
}

/// Check both files. Return sorted violations plus the item counts: todo
/// checkbox items and completed bullets.
fn analyze(todo: &str, completed: &str) -> (Vec<Report>, usize, usize) {
    let mut out = Vec::new();
    let mut used = HashSet::new();
    let (first, open) = scan_todo(todo);
    used.extend(first.iter().map(|&(_, line, _)| line));
    out.extend(first);

    // Rule 4, todo.md: identical checkbox bullet bodies.
    for (i, &(line, body)) in open.iter().enumerate() {
        if open[..i].iter().any(|&(_, b)| b == body) && used.insert(line) {
            out.push((TODO_PATH, line, MSG_DUP));
        }
    }

    let done = scan_completed(completed);

    // Rule 4, completed.md: identical bullet bodies.
    for (i, &(line, body)) in done.iter().enumerate() {
        if done[..i].iter().any(|&(_, b)| b == body) {
            out.push((COMPLETED_PATH, line, MSG_DUP));
        }
    }

    // Rule 5: a todo checkbox body that also appears in completed.md.
    for &(line, body) in &open {
        if done.iter().any(|&(_, b)| b == body) && used.insert(line) {
            out.push((TODO_PATH, line, MSG_CROSS));
        }
    }

    out.sort();
    (out, open.len(), done.len())
}

/// Read a task file; report a missing or unreadable file on error.
fn read_task_file(base: &Path, rel: &'static str, reports: &mut Vec<Report>) -> String {
    match fs::read_to_string(base.join(rel)) {
        Ok(text) => text,
        Err(_) => {
            reports.push((rel, 1, MSG_MISSING));
            String::new()
        }
    }
}

fn main() -> ExitCode {
    let root = match env::args().nth(1) {
        Some(arg) => arg,
        None => String::from("."),
    };
    let base = Path::new(&root);
    let mut reports = Vec::new();
    let todo = read_task_file(base, TODO_PATH, &mut reports);
    let completed = read_task_file(base, COMPLETED_PATH, &mut reports);
    if reports.is_empty() {
        let (extra, n_todo, n_done) = analyze(&todo, &completed);
        reports.extend(extra);
        if reports.is_empty() {
            println!("pass: tracker coherent ({n_todo} todo items, {n_done} completed items)");
            return ExitCode::SUCCESS;
        }
    }
    reports.sort();
    for (path, line, message) in &reports {
        println!("{path}:{line}: {message}");
    }
    println!("fail: {} violation(s) found", reports.len());
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use super::*;

    const COHERENT_TODO: &str = "\
## Phase 2 - Context engine
- [ ] alpha: first item.
- [~] beta: in progress.
- Checkboxes: `- [ ]` open, `- [x]` done, `- [~]` in progress.

## Wrap-up (complete 2026-01-01)
- A plain note, not a checkbox.
";

    const COHERENT_DONE: &str = "\
## Completed
- alpha shipped (abc1234).
- beta done (abc1234).
";

    /// Run the rules and return only the sorted violation reports.
    fn check(todo: &str, completed: &str) -> Vec<Report> {
        analyze(todo, completed).0
    }

    #[test]
    fn coherent_pair_passes_with_counts() {
        assert_eq!(analyze(COHERENT_TODO, COHERENT_DONE), (vec![], 2, 2));
    }

    #[test]
    fn legend_bullet_is_not_a_checkbox() {
        let todo = "- Checkboxes: `- [ ]` open, `- [x]` done (awaiting move).\n";
        assert_eq!(check(todo, ""), vec![]);
    }

    #[test]
    fn stale_done_item_flagged() {
        let todo = "- [x] finished but not moved\n";
        assert_eq!(
            check(todo, "- something else\n"),
            vec![(TODO_PATH, 1, MSG_STALE)]
        );
    }

    #[test]
    fn malformed_marker_flagged() {
        let todo = "- [q] wrong state letter\n";
        assert_eq!(check(todo, ""), vec![(TODO_PATH, 1, MSG_MARKER)]);
    }

    #[test]
    fn duplicate_within_todo_flagged() {
        let todo = "- [ ] same task\n- [ ] same task\n";
        assert_eq!(check(todo, ""), vec![(TODO_PATH, 2, MSG_DUP)]);
    }

    #[test]
    fn duplicate_within_completed_flagged() {
        let done = "- note one\n- note one\n";
        assert_eq!(
            check("- [ ] a task\n", done),
            vec![(COMPLETED_PATH, 2, MSG_DUP)]
        );
    }

    #[test]
    fn item_in_both_files_flagged_on_todo_line() {
        let todo = "- [ ] still listed task\n";
        assert_eq!(
            check(todo, "- still listed task\n"),
            vec![(TODO_PATH, 1, MSG_CROSS)]
        );
    }

    #[test]
    fn checkbox_under_complete_section_flagged() {
        let todo = "## Phase 9 (complete 2026-01-01)\n- [ ] stray open item\n";
        assert_eq!(check(todo, ""), vec![(TODO_PATH, 2, MSG_SECTION)]);
    }

    #[test]
    fn complete_section_ends_at_next_header() {
        // The `- [x]` fires the stale rule; the item under `## Open` is fine
        // because the completed region ended at that header.
        let todo = "## Alpha (complete 2026-01-01)\n- [x] done item\n## Open\n- [ ] open item\n";
        assert_eq!(check(todo, ""), vec![(TODO_PATH, 2, MSG_STALE)]);
    }

    #[test]
    fn multiple_violations_sorted_by_line() {
        let todo = "- [ ] task\n- [x] stale task\n- [q] malformed\n- [ ] task\n";
        assert_eq!(
            check(todo, "- stale task\n"),
            vec![
                (TODO_PATH, 2, MSG_STALE),
                (TODO_PATH, 3, MSG_MARKER),
                (TODO_PATH, 4, MSG_DUP),
            ]
        );
    }
}
