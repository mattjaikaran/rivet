//! Type annotations in the Python DSL.
//!
//! An annotation becomes a [`TypeRef`] plus two independent markers:
//! [`ParsedType::is_optional`], which wraps the rendered type in
//! `Option<...>`, and [`ParsedType::is_borrowed`], which makes a `str` field
//! take a lifetime and deserialize by borrowing from the request body.
//!
//! DTO references are not resolved here; the caller holds the class table.

use crate::diagnostic::Diagnostic;
use crate::parser::is_safe_identifier;
use rivet_core::ir::TypeRef;

/// A parsed type annotation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedType {
    pub(crate) type_ref: TypeRef,
    pub(crate) is_optional: bool,
    pub(crate) is_borrowed: bool,
}

impl ParsedType {
    /// A plain, present, owned type.
    fn owned(type_ref: TypeRef) -> Self {
        Self {
            type_ref,
            is_optional: false,
            is_borrowed: false,
        }
    }

    /// The same type behind `Optional[...]`.
    ///
    /// A borrowed field cannot be optional: `Option<&'a str>` needs the
    /// borrow and the wrapper to share one lifetime, which the generator does
    /// not render. Reject the combination where the user can see it.
    fn optional(self, annotation: &str, file: &str, line: usize) -> Result<Self, Diagnostic> {
        if self.is_borrowed {
            return Err(borrowed_error(
                annotation,
                "an optional field cannot borrow".to_string(),
                "declare the field as a plain `borrowed[str]`, or drop the borrow and use `Optional[str]`",
                file,
                line,
            ));
        }
        Ok(Self {
            is_optional: true,
            ..self
        })
    }
}

/// Parse a type annotation into a [`ParsedType`].
///
/// Accepts optional wrappers (`Optional[X]`, `X | None`), `borrowed[str]`,
/// `dict`/primitives, `List[T]` / `List[T, N]`, and references to DTO
/// classes.
pub(crate) fn parse_type_text(
    annotation: &str,
    file: &str,
    line: usize,
    inside_array: bool,
) -> Result<ParsedType, Diagnostic> {
    let text = annotation.trim();

    // `Optional[X]`
    if text.starts_with("Optional[") && text.ends_with(']') {
        let inner = &text[9..text.len() - 1];
        return parse_type_text(inner, file, line, inside_array)?.optional(text, file, line);
    }
    // `X | None` unions
    if let Some(inner) = text.strip_suffix("| None").map(str::trim) {
        return parse_type_text(inner, file, line, inside_array)?.optional(text, file, line);
    }
    if let Some(inner) = text.strip_suffix("None |").map(str::trim) {
        return parse_type_text(inner, file, line, inside_array)?.optional(text, file, line);
    }
    if text.contains('|') {
        return Err(Diagnostic::blocker(
            "E1002",
            format!("union type `{text}` is not supported; use `Optional[...]`"),
            "rewrite the annotation with `Optional[...]` or a single type, for example `Optional[str]`",
        )
        .located(file, line));
    }

    // `borrowed[str]`: the field deserializes by borrowing its text from the
    // request body instead of allocating a `String`.
    if let Some(inner) = borrowed_inner(text) {
        if inside_array {
            return Err(borrowed_error(
                text,
                "an array element cannot borrow".to_string(),
                "declare the whole field as `borrowed[str]`, or use `List[str]` so each value owns its text",
                file,
                line,
            ));
        }
        if inner.trim() != "str" {
            return Err(borrowed_error(
                text,
                format!(
                    "only `str` can borrow, and this annotation names `{}`",
                    inner.trim()
                ),
                "write the field as `borrowed[str]`, or use the owned type the field needs",
                file,
                line,
            ));
        }
        return Ok(ParsedType {
            type_ref: TypeRef::String,
            is_optional: false,
            is_borrowed: true,
        });
    }

    // `List[T]` or `List[T, N]` (the DSL drops the `typing.` prefix)
    if let Some(inner) = array_inner(text) {
        if inside_array {
            return Err(Diagnostic::blocker(
                "E1002",
                format!("nested array type `{text}` is not supported"),
                "flatten the annotation to a single array level, for example `List[dict]`",
            )
            .located(file, line));
        }
        let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
        let (element_text, len) = match parts.as_slice() {
            [element] => (*element, None),
            [element, size] => {
                let size = size.parse::<usize>().map_err(|_| {
                    Diagnostic::blocker(
                        "E1002",
                        format!("array size `{size}` is not a positive integer"),
                        "give the array a positive integer size, for example `List[float, 768]`",
                    )
                    .located(file, line)
                })?;
                (*element, Some(size))
            }
            _ => {
                return Err(Diagnostic::blocker(
                    "E1002",
                    format!("array type `{text}` must name an element type and an optional size"),
                    "write the array as `List[Element]` or `List[Element, Size]`, for example `List[str]`",
                )
                .located(file, line));
            }
        };
        let element = parse_type_text(element_text, file, line, true)?;
        return Ok(ParsedType::owned(TypeRef::Array {
            element: Box::new(element.type_ref),
            len,
        }));
    }

    let type_ref = match text {
        "str" => TypeRef::String,
        "bool" => TypeRef::Bool,
        "int" => TypeRef::Int,
        "float" => TypeRef::Float,
        "dict" => TypeRef::Json,
        "list" => TypeRef::Array {
            element: Box::new(TypeRef::Json),
            len: None,
        },
        other if other.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') => {
            // A DTO reference; existence is checked by the caller.
            if !is_safe_identifier(other) {
                return Err(Diagnostic::blocker(
                    "E1011",
                    format!("type name `{other}` is not a safe Rust identifier"),
                    "reference an existing DTO by a PascalCase name or use a builtin type",
                )
                .located(file, line));
            }
            TypeRef::Named(other.to_string())
        }
        other => {
            return Err(Diagnostic::blocker(
                "E1002",
                format!("type annotation `{other}` is not supported"),
                "use str, bool, int, float, dict, List[T], Optional[T], or a DTO class",
            )
            .located(file, line));
        }
    };
    Ok(ParsedType::owned(type_ref))
}

/// If the annotation is `List[...]` (optionally `typing.List[...]`), return
/// the inner text.
fn array_inner(text: &str) -> Option<&str> {
    let marker = "List[";
    let start = text.find(marker)?;
    let prefix = &text[..start];
    if !prefix.is_empty() && !prefix.ends_with('.') {
        return None;
    }
    let tail = &text[start + marker.len()..];
    tail.strip_suffix(']')
}

/// If the annotation is `borrowed[str]`, return the inner text.
///
/// The marker is exact, unlike `array_inner`'s `List[`: `borrowed` is a DSL
/// keyword here, not a module member of a typing module.
fn borrowed_inner(text: &str) -> Option<&str> {
    text.strip_prefix("borrowed[")?.strip_suffix(']')
}

/// An `E1014` diagnostic for a `borrowed[...]` annotation that cannot work.
fn borrowed_error(
    annotation: &str,
    problem: String,
    fix: &str,
    file: &str,
    line: usize,
) -> Diagnostic {
    Diagnostic::blocker(
        "E1014",
        format!("`{annotation}` is not a usable borrowed field: {problem}"),
        fix,
    )
    .located(file, line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_optional_and_array_annotations() {
        assert_eq!(
            parse_type_text("List[float, 768]", "app.py", 1, false).expect("parse"),
            ParsedType::owned(TypeRef::Array {
                element: Box::new(TypeRef::Float),
                len: Some(768),
            })
        );
        assert_eq!(
            parse_type_text("Optional[int]", "app.py", 1, false).expect("parse"),
            ParsedType {
                type_ref: TypeRef::Int,
                is_optional: true,
                is_borrowed: false,
            }
        );
        assert_eq!(
            parse_type_text("int | None", "app.py", 1, false).expect("parse"),
            ParsedType {
                type_ref: TypeRef::Int,
                is_optional: true,
                is_borrowed: false,
            }
        );
        assert_eq!(
            parse_type_text("dict", "app.py", 1, false).expect("parse"),
            ParsedType::owned(TypeRef::Json)
        );
    }

    #[test]
    fn a_borrowed_field_keeps_the_str_type_and_sets_the_marker() {
        assert_eq!(
            parse_type_text("borrowed[str]", "app.py", 7, false).expect("parse"),
            ParsedType {
                type_ref: TypeRef::String,
                is_optional: false,
                is_borrowed: true,
            }
        );
    }

    #[test]
    fn rejects_a_borrowed_annotation_that_cannot_borrow() {
        for annotation in [
            "borrowed[int]",
            "List[borrowed[str]]",
            "Optional[borrowed[str]]",
            "borrowed[str] | None",
        ] {
            let diagnostic =
                parse_type_text(annotation, "app.py", 1, false).expect_err("must fail");
            assert_eq!(diagnostic.error_code, "E1014", "{annotation}");
            assert!(!diagnostic.suggested_fix.is_empty(), "{annotation}");
        }
    }

    #[test]
    fn rejects_unknown_shapes() {
        let diagnostic = parse_type_text("Tuple[int]", "app.py", 1, false).expect_err("must fail");
        assert_eq!(diagnostic.error_code, "E1002");
        assert!(!diagnostic.suggested_fix.is_empty());
        let diagnostic =
            parse_type_text("List[List[int]]", "app.py", 1, false).expect_err("must fail");
        assert_eq!(diagnostic.error_code, "E1002");
        assert!(!diagnostic.suggested_fix.is_empty());
        let diagnostic = parse_type_text("int | str", "app.py", 1, false).expect_err("must fail");
        assert_eq!(diagnostic.error_code, "E1002");
        assert!(!diagnostic.suggested_fix.is_empty());
    }
}
