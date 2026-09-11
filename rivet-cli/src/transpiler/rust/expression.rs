//! The expression renderer: one IR [`Expr`] into Rust source.
//!
//! The renderer knows the type it is filling, so it can pick the right shape:
//! an owned `String` from a literal, a `Clone` on a parameter the body reads
//! twice, a `Vec` for a list. It is the half of codegen that a handler body
//! flows through, and the service layer and the channel both call it, so a
//! route's shape stays identical in every target.

use super::{Emitter, expr_kind, rust_str, type_label};
use crate::diagnostic::Diagnostic;
use rivet_core::ir::{Expr, FieldDefinition, StructDefinition, TypeRef};
use rivet_core::reserved;

impl Emitter<'_> {
    /// Render a named (DTO) response: a construction or a parameter.
    pub(super) fn render_named(
        &self,
        expr: &Expr,
        struct_def: &StructDefinition,
    ) -> Result<String, Diagnostic> {
        match expr {
            Expr::Construct { args, .. } => self.render_struct_expr(struct_def, args),
            Expr::Ident(var) => Ok(self.owned_ident(var)),
            _ => Err(Diagnostic::blocker(
                "E2002",
                format!(
                    "handler must return a `{}` construction or a parameter of that type",
                    struct_def.name
                ),
                format!(
                    "return the `{}` DTO as a construction, or return a request parameter of type `{}`",
                    struct_def.name, struct_def.name
                ),
            )
            .located("<generated>", 1)),
        }
    }

    /// Render a struct literal for a DTO construction.
    fn render_struct_expr(
        &self,
        struct_def: &StructDefinition,
        args: &[(String, Expr)],
    ) -> Result<String, Diagnostic> {
        let mut fields = String::new();
        for field in &struct_def.fields {
            let value = match args.iter().find(|(name, _)| name == &field.name) {
                Some((_, expr)) => self.render_field(expr, field)?,
                None if field.is_optional => "None".to_string(),
                None => {
                    return Err(Diagnostic::blocker(
                        "E2002",
                        format!(
                            "missing required field `{}` for `{}`",
                            field.name, struct_def.name
                        ),
                        format!(
                            "add the missing `{}=...` argument to the `{}` construction in the handler body",
                            field.name, struct_def.name
                        ),
                    )
                    .located("<generated>", 1));
                }
            };
            fields.push_str(&format!("        {}: {value},\n", field.name));
        }
        Ok(format!("{} {{\n{fields}    }}", struct_def.name))
    }

    /// Render one field value with the exact Rust type of the field.
    fn render_field(&self, expr: &Expr, field: &FieldDefinition) -> Result<String, Diagnostic> {
        // A borrowed field is a `&'a str`, so a literal renders as the
        // `&'static str` that coerces into it. Anything else would have to
        // name a buffer that outlives the DTO, which the DSL has no way to
        // write, so the generator rejects it instead of emitting a `String`
        // into a `&str` field.
        if field.is_borrowed {
            return match expr {
                Expr::Str(value) => Ok(rust_str(value)),
                _ => Err(Diagnostic::blocker(
                    "E2016",
                    format!(
                        "`{}` borrows from the request body, so it cannot be built with a {} value",
                        field.name,
                        expr_kind(expr)
                    ),
                    "pass a string literal, or declare the field as `str` so it owns its text",
                )
                .located("<generated>", 1)),
            };
        }
        if field.is_optional {
            if matches!(expr, Expr::Null) {
                return Ok("None".to_string());
            }
            let value = self.render_typed(expr, &field.type_ref)?;
            return Ok(format!("Some({value})"));
        }
        self.render_typed(expr, &field.type_ref)
    }

    /// Render an expression into the Rust type named by `ty`.
    fn render_typed(&self, expr: &Expr, ty: &TypeRef) -> Result<String, Diagnostic> {
        match ty {
            TypeRef::Json => self.render_value(expr),
            TypeRef::String => match expr {
                Expr::Str(value) => Ok(format!("{}.to_owned()", rust_str(value))),
                Expr::Ident(var) => Ok(self.owned_ident(var)),
                _ => Err(self.type_error(expr, ty)),
            },
            TypeRef::Int => match expr {
                Expr::Int(value) => Ok(format!("{value}i64")),
                Expr::Ident(var) => Ok(var.clone()), // i64 is Copy
                _ => Err(self.type_error(expr, ty)),
            },
            TypeRef::Float => match expr {
                Expr::Int(value) => Ok(value.to_string()), // literal widens to f64
                Expr::Float(value) => Ok(value.to_string()),
                Expr::Ident(var) => match self.params.get(var) {
                    Some(TypeRef::Int) => Ok(format!("{var} as f64")),
                    _ => Ok(var.clone()),
                },
                _ => Err(self.type_error(expr, ty)),
            },
            TypeRef::Bool => match expr {
                Expr::Bool(value) => Ok(value.to_string()),
                Expr::Ident(var) => Ok(var.clone()),
                _ => Err(self.type_error(expr, ty)),
            },
            TypeRef::Array { element, len } => match expr {
                Expr::Array(items) => {
                    let rendered = items
                        .iter()
                        .map(|item| self.render_typed(item, element))
                        .collect::<Result<Vec<_>, _>>()?;
                    match len {
                        // A fixed-size target takes an array literal, and
                        // its length has to match the declaration exactly:
                        // Rust would report the mismatch as a type error in
                        // the generated crate, which reads as a generator
                        // fault instead of a body mistake.
                        Some(size) if items.len() != *size => Err(Diagnostic::blocker(
                            "E2011",
                            format!(
                                "the list literal holds {} value(s); the fixed-size array is declared as `List[_, {size}]`",
                                items.len()
                            ),
                            format!(
                                "give the literal exactly {size} values, or declare the field as a plain `List[...]` without a size"
                            ),
                        )
                        .located("<generated>", 1)),
                        Some(_) => Ok(format!("[{}]", rendered.join(", "))),
                        None => Ok(format!("vec![{}]", rendered.join(", "))),
                    }
                }
                Expr::Ident(var) => Ok(self.owned_ident(var)),
                _ => Err(self.type_error(expr, ty)),
            },
            TypeRef::Named(name) => match expr {
                Expr::Construct { args, .. } => {
                    let struct_def = self.codegen.find_struct(name)?;
                    self.render_struct_expr(struct_def, args)
                }
                Expr::Ident(var) => Ok(self.owned_ident(var)),
                _ => Err(self.type_error(expr, ty)),
            },
        }
    }

    /// Render an expression of type `serde_json::Value`.
    pub(super) fn render_value(&self, expr: &Expr) -> Result<String, Diagnostic> {
        match expr {
            Expr::Null => Ok("serde_json::Value::Null".to_string()),
            Expr::Bool(value) => Ok(format!("serde_json::Value::Bool({value})")),
            Expr::Int(value) => Ok(format!("serde_json::Value::from({value}i64)")),
            Expr::Float(value) => Ok(format!("{}json_number({value})", reserved::PREFIX)),
            Expr::Str(value) => Ok(format!("serde_json::Value::from({})", rust_str(value))),
            Expr::Array(items) => {
                let rendered = items
                    .iter()
                    .map(|item| self.render_value(item))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(format!(
                    "serde_json::Value::Array(vec![{}])",
                    rendered.join(", ")
                ))
            }
            Expr::Object(entries) => {
                let rendered = entries
                    .iter()
                    .map(|(key, value)| {
                        Ok(format!(
                            "({}, {})",
                            rust_str(key),
                            self.render_value(value)?
                        ))
                    })
                    .collect::<Result<Vec<_>, Diagnostic>>()?;
                Ok(format!(
                    "{}json_obj(vec![{}])",
                    reserved::PREFIX,
                    rendered.join(", ")
                ))
            }
            Expr::Ident(var) => match self.params.get(var) {
                Some(TypeRef::Json) => Ok(self.owned_ident(var)),
                Some(_) => {
                    // Borrow and serialize: works for DTOs and primitives and
                    // is safe under repeated use.
                    Ok(format!(
                        "serde_json::to_value(&{var}).expect(\"request value serialization cannot fail\")"
                    ))
                }
                None => Err(Diagnostic::blocker(
                    "E2002",
                    format!("`{var}` is not a parameter of this handler"),
                    format!("make `{var}` a request parameter of the handler, or replace the reference with a literal"),
                )
                .located("<generated>", 1)),
            },
            Expr::Construct { .. } => Err(Diagnostic::blocker(
                "E2002",
                "DTO construction nested inside a JSON value is not supported yet",
                "return the DTO construction as the handler's declared response type instead of nesting it inside the JSON value",
            )
            .located("<generated>", 1)),
        }
    }

    /// A parameter of a non-`Copy` type is moved on its first use; render a
    /// clone on every use when the handler body references it more than once.
    pub(super) fn owned_ident(&self, var: &str) -> String {
        if self.counts.get(var).copied().unwrap_or(0) > 1 {
            format!("{var}.clone()")
        } else {
            var.to_string()
        }
    }

    fn type_error(&self, expr: &Expr, ty: &TypeRef) -> Diagnostic {
        Diagnostic::blocker(
            "E2002",
            format!(
                "a {} value cannot satisfy a field of type `{}`",
                expr_kind(expr),
                type_label(ty)
            ),
            format!(
                "return a value of the field's declared type `{}` (a literal, a request parameter of that type, or a DTO construction)",
                type_label(ty)
            ),
        )
        .located("<generated>", 1)
    }
}
