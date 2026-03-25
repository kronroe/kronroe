//! Minimal JSON serializer for the Kronroe append-log.
//!
//! Zero external dependencies. Only handles the types Kronroe actually stores:
//! strings, numbers, booleans, null, arrays, and objects with string keys.
//! Output is compact (no whitespace).

use std::io::{self, Write};

/// Write a JSON-escaped string (with surrounding quotes) to `w`.
pub fn write_string(w: &mut impl Write, s: &str) -> io::Result<()> {
    w.write_all(b"\"")?;
    for byte in s.bytes() {
        match byte {
            b'"' => w.write_all(b"\\\"")?,
            b'\\' => w.write_all(b"\\\\")?,
            b'\n' => w.write_all(b"\\n")?,
            b'\r' => w.write_all(b"\\r")?,
            b'\t' => w.write_all(b"\\t")?,
            // Control characters U+0000..U+001F (except those handled above)
            0x00..=0x1f => write!(w, "\\u{byte:04x}")?,
            _ => w.write_all(&[byte])?,
        }
    }
    w.write_all(b"\"")
}

/// Write an f64 to `w` using the same format as serde_json.
///
/// NaN and Infinity are not valid JSON — Kronroe handles these cases
/// at a higher level (e.g. `PredicateVolatility` encodes infinity as `"inf"`).
pub fn write_f64(w: &mut impl Write, n: f64) -> io::Result<()> {
    // Integers are written without a decimal point, matching serde_json.
    if n.fract() == 0.0 && n.is_finite() && n.abs() < (1_i64 << 53) as f64 {
        write!(w, "{}", n as i64)
    } else {
        // Use Rust's default Display, which matches serde_json for most cases.
        // For precise round-trip fidelity we use enough decimal digits.
        write!(w, "{n}")
    }
}

/// Write an f32 to `w`.
pub fn write_f32(w: &mut impl Write, n: f32) -> io::Result<()> {
    if n.fract() == 0.0 && n.is_finite() && n.abs() < (1_i64 << 24) as f32 {
        write!(w, "{}", n as i32)
    } else {
        write!(w, "{n}")
    }
}

/// Write a JSON null.
pub fn write_null(w: &mut impl Write) -> io::Result<()> {
    w.write_all(b"null")
}

/// Write a JSON boolean.
pub fn write_bool(w: &mut impl Write, b: bool) -> io::Result<()> {
    w.write_all(if b { b"true" } else { b"false" })
}

/// Helper: write `"key":value` where value is a string.
pub fn write_kv_string(w: &mut impl Write, key: &str, value: &str) -> io::Result<()> {
    write_string(w, key)?;
    w.write_all(b":")?;
    write_string(w, value)
}

/// Helper: write `"key":value` where value is an f64.
#[cfg_attr(not(test), allow(dead_code))]
pub fn write_kv_f64(w: &mut impl Write, key: &str, value: f64) -> io::Result<()> {
    write_string(w, key)?;
    w.write_all(b":")?;
    write_f64(w, value)
}

/// Helper: write `"key":value` where value is an f32.
pub fn write_kv_f32(w: &mut impl Write, key: &str, value: f32) -> io::Result<()> {
    write_string(w, key)?;
    w.write_all(b":")?;
    write_f32(w, value)
}

/// Helper: write `"key":value` where value is a u64.
pub fn write_kv_u64(w: &mut impl Write, key: &str, value: u64) -> io::Result<()> {
    write_string(w, key)?;
    w.write_all(b":")?;
    write!(w, "{value}")
}

/// Helper: write `"key":value` where value is a bool.
#[cfg_attr(not(test), allow(dead_code))]
pub fn write_kv_bool(w: &mut impl Write, key: &str, value: bool) -> io::Result<()> {
    write_string(w, key)?;
    w.write_all(b":")?;
    write_bool(w, value)
}

/// Helper: write `"key":null` or `"key":"value"` for an Option<String>.
pub fn write_kv_option_string(
    w: &mut impl Write,
    key: &str,
    value: &Option<String>,
) -> io::Result<()> {
    write_string(w, key)?;
    w.write_all(b":")?;
    match value {
        Some(s) => write_string(w, s),
        None => write_null(w),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn ser(f: impl Fn(&mut Vec<u8>) -> io::Result<()>) -> String {
        let mut buf = Vec::new();
        f(&mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn string_escaping() {
        assert_eq!(ser(|w| write_string(w, "hello")), r#""hello""#);
        assert_eq!(ser(|w| write_string(w, "a\"b")), r#""a\"b""#);
        assert_eq!(ser(|w| write_string(w, "a\\b")), r#""a\\b""#);
        assert_eq!(ser(|w| write_string(w, "a\nb")), r#""a\nb""#);
        assert_eq!(ser(|w| write_string(w, "a\tb")), r#""a\tb""#);
        assert_eq!(ser(|w| write_string(w, "a\x00b")), "\"a\\u0000b\"");
    }

    #[test]
    fn number_formatting() {
        assert_eq!(ser(|w| write_f64(w, 42.0)), "42");
        assert_eq!(ser(|w| write_f64(w, -1.0)), "-1");
        assert_eq!(ser(|w| write_f64(w, 3.14)), "3.14");
        assert_eq!(ser(|w| write_f64(w, 0.0)), "0");
    }

    #[test]
    fn f32_formatting() {
        assert_eq!(ser(|w| write_f32(w, 1.0)), "1");
        assert_eq!(ser(|w| write_f32(w, 0.85)), "0.85");
    }

    #[test]
    fn bool_and_null() {
        assert_eq!(ser(|w| write_bool(w, true)), "true");
        assert_eq!(ser(|w| write_bool(w, false)), "false");
        assert_eq!(ser(|w| write_null(w)), "null");
    }
}
