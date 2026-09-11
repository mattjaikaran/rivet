//! The names the generated crate owns.
//!
//! The generated crate is one flat Rust namespace. User-derived identifiers
//! (handler function names, DTO struct names) and generator-owned identifiers
//! (modules, runtime helpers, imported types) live side by side, so a DSL name
//! that reuses a generator name makes the emitted crate fail to compile — and
//! cargo reports the collision against generated code the user never wrote.
//!
//! Two rules keep the two sets apart:
//!
//! - every generator-owned *symbol* carries [`PREFIX`], so a helper the
//!   generator adds later cannot collide with a name a user picks;
//! - the names the crate root must hold (the pillar modules, the entry point,
//!   the types the emitter writes, the crate paths) are listed in
//!   [`RESERVED`], and the front end rejects a DTO that reuses one.
//!
//! The generator constants and the reserved list live together, so a rename
//! cannot drift from the check: [`RESERVED`] holds every name in [`MODULES`],
//! and a unit test fails when the two fall out of step.

/// The module the generated handlers live in.
///
/// Both targets namespace their handlers, so a handler name never lands at
/// crate root: the native crate registers `handlers::ping::<channel::InProcess>`
/// and the WebAssembly target reaches the same logic through `service::ping`.
pub const HANDLERS_MODULE: &str = "handlers";

/// The module the transport-free route logic lives in.
pub const SERVICE_MODULE: &str = "service";

/// The module the service channel lives in.
pub const CHANNEL_MODULE: &str = "channel";

/// The module the admin panel lives in.
pub const ADMIN_MODULE: &str = "admin";

/// The module the embedded assets live in.
pub const ASSETS_MODULE: &str = "assets";

/// The module the service-registry client lives in.
pub const DISCOVERY_MODULE: &str = "discovery";

/// Every module the generated crate declares at its root.
pub const MODULES: &[&str] = &[
    HANDLERS_MODULE,
    SERVICE_MODULE,
    CHANNEL_MODULE,
    ADMIN_MODULE,
    ASSETS_MODULE,
    DISCOVERY_MODULE,
];

/// The prefix every generator-owned symbol carries.
pub const PREFIX: &str = "rivet_";

/// The same prefix in upper case, for the generator's constants.
pub const CONST_PREFIX: &str = "RIVET_";

/// Every name the generated code writes at crate root.
///
/// The front end rejects a **DTO** that reuses one, because a DTO becomes a
/// crate-root struct and shares the namespace with these. A **handler** name
/// is not checked against this list: after the handlers move into
/// [`HANDLERS_MODULE`], a handler is a local function that shadows a glob
/// import without error.
pub const RESERVED: &[&str] = &[
    // The modules the generated crate declares. Pillar 02 documents `service`
    // and `channel`; the rest are internal but still occupy the crate root.
    HANDLERS_MODULE,
    SERVICE_MODULE,
    CHANNEL_MODULE,
    ADMIN_MODULE,
    ASSETS_MODULE,
    DISCOVERY_MODULE,
    // The crate root already holds the entry point.
    "main",
    // The types the `use axum::…` line and the emitter write by name.
    "Json",
    "State",
    "Router",
    "String",
    "Vec",
    "Option",
    "bool",
    "i64",
    "f64",
    // The paths the generated code writes by name.
    "serde",
    "serde_json",
    "axum",
    "tokio",
    "tracing",
    "std",
];

/// Whether `name` carries the generator's reserved prefix.
///
/// Both cases count: the generator names functions and types with [`PREFIX`]
/// and constants with [`CONST_PREFIX`].
pub fn has_prefix(name: &str) -> bool {
    name.starts_with(PREFIX) || name.starts_with(CONST_PREFIX)
}

/// Whether a DTO may not use `name`.
///
/// `Option` is reserved even though it compiles today: the emitter writes
/// `Option<{base}>` only for an optional field, so the rule would otherwise
/// depend on an unrelated field of the DTO. Reserving it unconditionally
/// keeps the answer predictable instead of permissive.
pub fn is_reserved_dto(name: &str) -> bool {
    RESERVED.contains(&name) || has_prefix(name)
}

/// Whether a handler may not use `name`.
///
/// A handler becomes a function inside [`HANDLERS_MODULE`], where a local
/// item shadows a glob import in silence, so only the prefix is reserved —
/// that keeps the generator free to add a `rivet_*` helper later.
pub fn is_reserved_handler(name: &str) -> bool {
    has_prefix(name)
}

/// The names a generated handler binds as parameters.
///
/// Rust rejects two parameters bound to one name (`E0415`), so a handler
/// parameter that reuses one of these does not compile — and cargo reports
/// the error against generated code the user never wrote. The generator
/// binds `channel` for the service channel in every handler, and `bytes` for
/// the raw body of a route whose DTO borrows from it.
///
/// Only a parameter is bound this way. A value read *from* a parameter is a
/// `let` in the handler body, where shadowing is legal, so those names need
/// no reservation.
pub const HANDLER_PARAMETERS: &[&str] = &["channel", "bytes"];

/// Whether a handler parameter may not use `name`.
pub fn is_reserved_parameter(name: &str) -> bool {
    HANDLER_PARAMETERS.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reserved_list_holds_every_generated_module() {
        for module in MODULES {
            assert!(
                RESERVED.contains(module),
                "`{module}` is a module the generated crate declares, so RESERVED must hold it"
            );
        }
    }

    #[test]
    fn the_prefix_covers_both_cases() {
        assert!(is_reserved_handler("rivet_x"));
        assert!(is_reserved_handler("RIVET_DECLARED_PATHS"));
        assert!(is_reserved_dto("rivet_fixed_array"));
        assert!(is_reserved_dto("RIVET_DECLARED_PATHS"));
        assert!(!is_reserved_handler("service"));
        assert!(!is_reserved_dto("fixed_array"));
        assert!(!is_reserved_dto("rivetfree"));
    }

    #[test]
    fn the_reserved_list_names_the_crate_root() {
        for name in ["main", "Json", "State", "Router", "String", "Vec", "Option"] {
            assert!(is_reserved_dto(name), "`{name}` must stay reserved");
        }
        assert!(!is_reserved_dto("OrderCreate"));
    }
}
