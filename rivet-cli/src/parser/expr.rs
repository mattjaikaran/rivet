//! Lowering of Python handler-body expressions into IR [`Expr`] values.
//!
//! Phase 0 supports a documented subset of Python expressions:
//!
//! - literals: `None`, `True`, `False`, integers, floats, strings, lists,
//!   dictionaries
//! - references to the handler's request parameter
//! - DTO construction: `OrderResponse(status="ok")`
//!
//! Anything else (calls, attribute access, arithmetic, comprehensions,
//! f-strings, ...) fails with a diagnostic that names the construct. Failing
//! loudly is deliberate: a silent mistranslation of business logic would be
//! worse than an error.

use crate::diagnostic::Diagnostic;
use crate::parser::NamedChildren;
use rivet_core::ir::Expr;
use tree_sitter::Node;

/// Translate a Python expression node into an IR expression.
///
/// Diagnostics carry a line but no file; the caller re-anchors them to the
/// real file name.
pub fn translate(node: &Node<'_>, source: &str) -> Result<Expr, Diagnostic> {
    match node.kind() {
        "none" => Ok(Expr::Null),
        "true" => Ok(Expr::Bool(true)),
        "false" => Ok(Expr::Bool(false)),
        "integer" => {
            let text = text(node, source);
            text.parse::<i64>().map(Expr::Int).map_err(|_| {
                at(
                    node,
                    "E1007",
                    format!("integer literal `{text}` is too large for the target"),
                    None,
                )
            })
        }
        "float" => {
            let text = normalize_float(text(node, source));
            match text.parse::<f64>() {
                Ok(value) if value.is_finite() => Ok(Expr::Float(value)),
                Ok(_) => Err(at(
                    node,
                    "E1007",
                    format!("float literal `{text}` is not finite"),
                    None,
                )),
                Err(_) => Err(at(
                    node,
                    "E1007",
                    format!("float literal `{text}` is not valid"),
                    None,
                )),
            }
        }
        "string" => {
            let literal = text(node, source);
            match decode_string(literal) {
                Ok(value) => Ok(Expr::Str(value)),
                Err(reason) => Err(at(node, "E1007", reason, None)),
            }
        }
        "concatenated_string" => Err(unsupported(
            node,
            "adjacent string literals are not supported; use a single literal",
        )),
        "list" => {
            let mut items = Vec::new();
            for child in node.named_children_all() {
                items.push(translate(&child, source)?);
            }
            Ok(Expr::Array(items))
        }
        "dictionary" => {
            let mut pairs = Vec::new();
            for child in node.named_children_all() {
                if child.kind() != "pair" {
                    return Err(unsupported(
                        node,
                        "dictionary unpacking (`**`) is not supported",
                    ));
                }
                let key = child
                    .child_by_field_name("key")
                    .ok_or_else(|| at(&child, "E1007", "dictionary entry without a key", None))?;
                let value = child
                    .child_by_field_name("value")
                    .ok_or_else(|| at(&child, "E1007", "dictionary entry without a value", None))?;
                pairs.push((string_key(&key, source)?, translate(&value, source)?));
            }
            Ok(Expr::Object(pairs))
        }
        "identifier" => Ok(Expr::Ident(text(node, source).to_string())),
        "call" => {
            let function = node
                .child_by_field_name("function")
                .ok_or_else(|| at(node, "E1007", "call without a callee", None))?;
            if function.kind() != "identifier" {
                return Err(unsupported(
                    node,
                    "only DTO constructors may be called; calls on objects and modules are not supported yet",
                ));
            }
            let ty = text(&function, source).to_string();
            let mut args = Vec::new();
            if let Some(arguments) = node.child_by_field_name("arguments") {
                for child in arguments.named_children_all() {
                    if child.kind() != "keyword_argument" {
                        return Err(unsupported(
                            &child,
                            "DTO constructors take keyword arguments only, e.g. OrderResponse(status=\"ok\")",
                        ));
                    }
                    let name = child
                        .child_by_field_name("name")
                        .map(|n| text(&n, source))
                        .ok_or_else(|| {
                            at(&child, "E1007", "keyword argument without a name", None)
                        })?;
                    let value = child.child_by_field_name("value").ok_or_else(|| {
                        at(&child, "E1007", "keyword argument without a value", None)
                    })?;
                    args.push((name.to_string(), translate(&value, source)?));
                }
            }
            Ok(Expr::Construct { ty, args })
        }
        "unary_operator" => translate_unary(node, source),
        "attribute" => Err(unsupported(
            node,
            "attribute access (`x.y`) is not supported in handler bodies yet",
        )),
        other => Err(unsupported(
            node,
            &format!("the `{other}` expression is not part of the supported subset"),
        )),
    }
}

/// Translate a unary operator, supporting negated numeric literals only.
fn translate_unary(node: &Node<'_>, source: &str) -> Result<Expr, Diagnostic> {
    let operator = node.child(0).map(|c| text(&c, source)).unwrap_or("");
    let operand = node
        .named_children_all()
        .first()
        .copied()
        .ok_or_else(|| at(node, "E1007", "unary operator without an operand", None))?;
    match operator {
        "-" => match translate(&operand, source)? {
            Expr::Int(value) => Ok(Expr::Int(-value)),
            Expr::Float(value) => Ok(Expr::Float(-value)),
            _ => Err(unsupported(
                node,
                "negation is supported for numeric literals only",
            )),
        },
        "+" => translate(&operand, source),
        other => Err(unsupported(
            node,
            &format!("the unary operator `{other}` is not supported"),
        )),
    }
}

/// A dictionary key must be a string literal.
fn string_key(node: &Node<'_>, source: &str) -> Result<String, Diagnostic> {
    if node.kind() == "string" {
        return decode_string(text(node, source)).map_err(|reason| at(node, "E1007", reason, None));
    }
    Err(unsupported(node, "dictionary keys must be string literals"))
}

/// Build a diagnostic anchored at a node's line.
fn at(node: &Node<'_>, code: &str, message: impl Into<String>, fix: Option<&str>) -> Diagnostic {
    let mut diagnostic = Diagnostic::blocker(code, message);
    diagnostic.line = Some(node.start_position().row + 1);
    diagnostic.suggested_fix = fix.map(str::to_string);
    diagnostic
}

fn unsupported(node: &Node<'_>, reason: &str) -> Diagnostic {
    at(
        node,
        "E1007",
        reason.to_string(),
        Some(
            "simplify the handler body to the supported subset: literals, lists, dictionaries, request parameters, and DTO constructors",
        ),
    )
}

fn text<'a>(node: &Node<'_>, source: &'a str) -> &'a str {
    node.utf8_text(source.as_bytes()).unwrap_or("?")
}

/// Python accepts `1e3` and `.5`; Rust accepts the former but not a leading
/// dot. Normalize to text Rust can parse.
fn normalize_float(literal: &str) -> String {
    if literal.starts_with('.') {
        format!("0{literal}")
    } else {
        literal.to_string()
    }
}

/// Decode a Python string literal (with prefixes and escapes) into its value.
pub fn decode_string(literal: &str) -> Result<String, String> {
    let bytes = literal.as_bytes();
    let mut cursor = 0;
    let mut raw = false;

    // Optional prefix letters, in any order: r, u, b, f (and uppercase).
    while cursor < bytes.len() && bytes[cursor].is_ascii_alphabetic() {
        match bytes[cursor].to_ascii_lowercase() {
            b'r' => raw = true,
            b'f' => return Err("f-strings are not supported in handler bodies".to_string()),
            b'b' => return Err("bytes literals are not supported; use a plain string".to_string()),
            b'u' => {} // plain unicode string
            _ => {
                return Err(format!(
                    "unknown string prefix `{}`",
                    &literal[..cursor + 1]
                ));
            }
        }
        cursor += 1;
    }

    let content = split_quotes(&literal[cursor..])?;
    if raw {
        return Ok(content.to_string());
    }
    decode_escapes(content)
}

/// Strip the opening and closing quote from a string literal.
fn split_quotes(literal: &str) -> Result<&str, String> {
    if literal.len() < 2 {
        return Err("string literal is too short".to_string());
    }
    if literal.starts_with("'''") || literal.starts_with("\"\"\"") {
        let quote = &literal[..3];
        let rest = &literal[3..];
        if rest.len() < 3 {
            return Err("unterminated string literal".to_string());
        }
        if !rest.ends_with(quote) {
            return Err("unterminated multi-line string literal".to_string());
        }
        return Ok(&rest[..rest.len() - 3]);
    }
    let quote = &literal[..1];
    if !literal.ends_with(quote) {
        return Err("unterminated string literal".to_string());
    }
    Ok(&literal[1..literal.len() - 1])
}

/// Decode backslash escapes in the content of a string literal. Python escape
/// rules apply; an escape Python would keep literally is kept.
fn decode_escapes(content: &str) -> Result<String, String> {
    let mut out = String::with_capacity(content.len());
    let mut chars = content.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        let Some(escaped) = chars.next() else {
            return Err("string ends with a dangling backslash".to_string());
        };
        match escaped {
            '\\' => out.push('\\'),
            '\'' => out.push('\''),
            '"' => out.push('"'),
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            'b' => out.push('\u{0008}'),
            'f' => out.push('\u{000C}'),
            'v' => out.push('\u{000B}'),
            'a' => out.push('\u{0007}'),
            '0' => out.push('\0'),
            'x' => {
                let hex = take(&mut chars, 2).ok_or("incomplete `\\x` escape")?;
                let value = u32::from_str_radix(&hex, 16)
                    .map_err(|_| format!("invalid `\\x{hex}` escape"))?;
                push_codepoint(&mut out, value)?;
            }
            'u' => {
                let hex = take(&mut chars, 4).ok_or("incomplete `\\u` escape")?;
                let value = u32::from_str_radix(&hex, 16)
                    .map_err(|_| format!("invalid `\\u{hex}` escape"))?;
                push_codepoint(&mut out, value)?;
            }
            'U' => {
                let hex = take(&mut chars, 8).ok_or("incomplete `\\U` escape")?;
                let value = u32::from_str_radix(&hex, 16)
                    .map_err(|_| format!("invalid `\\U{hex}` escape"))?;
                push_codepoint(&mut out, value)?;
            }
            '\n' => {} // line continuation
            digit if digit.is_ascii_digit() => {
                // Octal escape: up to two more octal digits.
                let mut oct = String::from(digit);
                for _ in 0..2 {
                    let Some(&next) = chars.peek() else { break };
                    if !next.is_ascii_digit() || next > '7' {
                        break;
                    }
                    oct.push(next);
                    chars.next();
                }
                let value = u32::from_str_radix(&oct, 8)
                    .map_err(|_| format!("invalid octal escape `\\{oct}`"))?;
                push_codepoint(&mut out, value)?;
            }
            other => out.push(other), // unknown escapes keep the character
        }
    }
    Ok(out)
}

fn take(chars: &mut std::iter::Peekable<std::str::Chars<'_>>, count: usize) -> Option<String> {
    let mut out = String::with_capacity(count);
    for _ in 0..count {
        out.push(chars.next()?);
    }
    Some(out)
}

fn push_codepoint(out: &mut String, value: u32) -> Result<(), String> {
    match char::from_u32(value) {
        Some(ch) => {
            out.push(ch);
            Ok(())
        }
        None => Err(format!("escape produces invalid code point U+{value:04X}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_plain_and_escaped_strings() {
        assert_eq!(decode_string("\"hello\"").expect("ok"), "hello");
        assert_eq!(decode_string("'hi'").expect("ok"), "hi");
        assert_eq!(decode_string(r#""a\nb""#).expect("ok"), "a\nb");
        assert_eq!(decode_string(r#"'\t'"#).expect("ok"), "\t");
        assert_eq!(decode_string(r#""\u00e9""#).expect("ok"), "é");
        assert_eq!(decode_string(r#""\U0001F600""#).expect("ok"), "😀");
        assert_eq!(decode_string(r#""\x41""#).expect("ok"), "A");
        assert_eq!(decode_string(r#""\101""#).expect("ok"), "A");
        assert_eq!(decode_string(r#""\\""#).expect("ok"), "\\");
        assert_eq!(decode_string(r#""\q""#).expect("ok"), "q");
        assert_eq!(decode_string(r#"r"a\nb""#).expect("ok"), "a\\nb");
        assert_eq!(decode_string(r#"u"caf\u00e9""#).expect("ok"), "café");
    }

    #[test]
    fn rejects_f_strings_and_bytes() {
        assert!(decode_string(r#"f"value {x}""#).is_err());
        assert!(decode_string(r#"b"bytes""#).is_err());
    }

    #[test]
    fn decodes_multiline_strings() {
        let value = decode_string("\"\"\"line one\nline two\"\"\"").expect("ok");
        assert_eq!(value, "line one\nline two");
    }

    #[test]
    fn normalizes_leading_dot_floats() {
        assert_eq!(normalize_float(".5"), "0.5");
        assert_eq!(normalize_float("1e3"), "1e3");
    }
}
