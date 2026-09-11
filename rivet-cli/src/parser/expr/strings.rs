//! Python string-literal decoding.
//!
//! Separated from the expression translator because escaping is its own
//! concern: the translator decides what kind of node it sees, and this module
//! decides what text a literal holds.

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
