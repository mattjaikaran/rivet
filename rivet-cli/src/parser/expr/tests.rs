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
