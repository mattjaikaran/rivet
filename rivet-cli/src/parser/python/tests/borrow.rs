//! The `borrowed[str]` parser rows.
//!
//! The borrow marker is recorded on a DTO field; a bare parameter or a return
//! annotation has no request body to point into, so both are rejected here,
//! where the diagnostic can carry the real file and line.

use super::*;

/// A `borrowed[str]` field is a slice of the request body, so only a DTO can
/// hold it.
#[test]
fn a_borrowed_annotation_outside_a_dto_is_rejected() {
    for (label, source) in [
        (
            "parameter",
            "from rivet import api\n\n@api.post(\"/notes\", stories=[\"US-1\"])\ndef create(request: borrowed[str]) -> dict:\n    return {\"echo\": request}\n",
        ),
        (
            "return type",
            "from rivet import api\n\n@api.post(\"/notes\", stories=[\"US-1\"])\ndef create() -> borrowed[str]:\n    return {\"a\": 1}\n",
        ),
    ] {
        let diagnostic = parse(source).expect_err(&format!("a borrowed {label} cannot work"));
        assert_eq!(diagnostic.error_code, "E1014", "{label}");
        assert!(!diagnostic.suggested_fix.is_empty(), "{label}");
    }
}

/// A DTO field carries the borrow: the parser records it on the field, and the
/// generator decides whether the project opted in.
#[test]
fn a_borrowed_dto_field_records_the_marker() {
    let source = "from rivet import api\n\nclass Note:\n    text: borrowed[str]\n\n@api.post(\"/notes\", stories=[\"US-1\"])\ndef create(request: Note) -> dict:\n    return {\"echo\": request}\n";
    let blueprint = parse(source).expect("parse");
    let note = &blueprint.structs[0];
    assert_eq!(note.name, "Note");
    assert!(note.fields[0].is_borrowed);
    assert_eq!(note.fields[0].type_ref, TypeRef::String);
    assert!(!note.fields[0].is_optional);
}
