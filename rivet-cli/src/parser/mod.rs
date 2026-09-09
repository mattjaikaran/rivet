//! Language front ends: DSL source files to a [`ServiceBlueprint`].
//!
//! Each supported language lives in its own module. The Python module is the
//! phase-0 front end; a TypeScript module slots in beside it later. Every
//! front end must emit the same IR and must fail with a [`Diagnostic`] rather
//! than guess.
//!
//! Shared node helpers used by every front end live here; grammar-specific
//! parsing lives in the per-language modules.

mod body;
mod decorator;
mod expr;
pub mod python;
mod signature;
mod types;
mod validate;

use tree_sitter::Node;

/// Collect the named children of a node.
///
/// tree-sitter's cursor-based iterator cannot outlive the local cursor, so
/// front ends collect into a small vector instead. Parsing is not a hot path.
pub(crate) trait NamedChildren {
    fn named_children_all(&self) -> Vec<Node<'_>>;
}

impl NamedChildren for Node<'_> {
    fn named_children_all(&self) -> Vec<Node<'_>> {
        let mut cursor = self.walk();
        self.named_children(&mut cursor).collect()
    }
}

/// The 1-based source line a node starts on, for diagnostics.
pub(crate) fn line_of(node: &Node<'_>) -> usize {
    node.start_position().row + 1
}

/// The source text spanned by a node.
pub(crate) fn node_text<'a>(node: &Node<'_>, source: &'a str) -> &'a str {
    node.utf8_text(source.as_bytes()).unwrap_or("?")
}

/// Docstrings are `expression_statement`s whose only named child is a string.
pub(crate) fn is_docstring(statement: &Node<'_>) -> bool {
    let children = statement.named_children_all();
    match (children.first(), children.get(1)) {
        (Some(child), None) => child.kind() == "string",
        _ => false,
    }
}

/// True when `name` is a plain-ASCII identifier that is not a Rust keyword.
/// The generator emits identifiers verbatim, so they must be valid Rust.
pub(crate) fn is_safe_identifier(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    let head_ok = first == '_' || first.is_ascii_alphabetic();
    let tail_ok = chars.all(|c| c == '_' || c.is_ascii_alphanumeric());
    head_ok && tail_ok && !RUST_KEYWORDS.contains(&name)
}

const RUST_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
    "unsafe", "use", "where", "while", "yield",
];
