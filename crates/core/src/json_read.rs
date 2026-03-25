//! Minimal recursive-descent JSON parser for the Kronroe append-log.
//!
//! Zero external dependencies. Parses JSON into a [`JsonValue`] DOM that
//! the per-type `from_json` methods can inspect. Only handles standard JSON
//! (RFC 8259) — no comments, no trailing commas, no NaN/Infinity literals.

use std::collections::BTreeMap;

/// A parsed JSON value.
#[derive(Debug, Clone, PartialEq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Number(f64),
    Str(String),
    Array(Vec<JsonValue>),
    Object(BTreeMap<String, JsonValue>),
}

impl JsonValue {
    /// Parse a JSON value from a byte slice.
    pub fn parse(input: &[u8]) -> Result<Self, ParseError> {
        let input =
            std::str::from_utf8(input).map_err(|e| ParseError(format!("invalid UTF-8: {e}")))?;
        let mut parser = Parser::new(input);
        let value = parser.parse_value()?;
        parser.skip_whitespace();
        if parser.pos < parser.input.len() {
            return Err(ParseError(format!(
                "trailing data at position {}",
                parser.pos
            )));
        }
        Ok(value)
    }

    /// Parse a JSON value from a string.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn parse_str(input: &str) -> Result<Self, ParseError> {
        Self::parse(input.as_bytes())
    }

    // -- Accessor helpers for ergonomic field extraction --

    /// Get a field from a JSON object.
    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        match self {
            JsonValue::Object(map) => map.get(key),
            _ => None,
        }
    }

    /// Extract a string value.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsonValue::Str(s) => Some(s),
            _ => None,
        }
    }

    /// Extract a number value.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            JsonValue::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// Extract a boolean value.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            JsonValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Extract an array value.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn as_array(&self) -> Option<&[JsonValue]> {
        match self {
            JsonValue::Array(a) => Some(a),
            _ => None,
        }
    }

    /// Returns true if this value is null.
    pub fn is_null(&self) -> bool {
        matches!(self, JsonValue::Null)
    }

    /// Extract a u64 from a number value.
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            JsonValue::Number(n) if *n >= 0.0 && n.fract() == 0.0 => Some(*n as u64),
            _ => None,
        }
    }

    /// Extract an f32 from a number value.
    pub fn as_f32(&self) -> Option<f32> {
        match self {
            JsonValue::Number(n) => Some(*n as f32),
            _ => None,
        }
    }
}

/// A JSON parse error.
#[derive(Debug, Clone)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JSON parse error: {}", self.0)
    }
}

impl std::error::Error for ParseError {}

// ---------------------------------------------------------------------------
// Recursive-descent parser
// ---------------------------------------------------------------------------

struct Parser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn parse_value(&mut self) -> Result<JsonValue, ParseError> {
        self.skip_whitespace();
        match self.peek() {
            Some(b'"') => self.parse_string().map(JsonValue::Str),
            Some(b'{') => self.parse_object(),
            Some(b'[') => self.parse_array(),
            Some(b't') => self.parse_literal("true", JsonValue::Bool(true)),
            Some(b'f') => self.parse_literal("false", JsonValue::Bool(false)),
            Some(b'n') => self.parse_literal("null", JsonValue::Null),
            Some(c) if c == b'-' || c.is_ascii_digit() => self.parse_number(),
            Some(c) => Err(self.error(format!("unexpected character '{}'", c as char))),
            None => Err(self.error("unexpected end of input".into())),
        }
    }

    fn parse_string(&mut self) -> Result<String, ParseError> {
        self.expect(b'"')?;
        let mut result = String::new();
        loop {
            // Fast path: scan for the next special character (quote, backslash)
            // and copy the entire literal run at once. This correctly handles
            // multi-byte UTF-8 because we slice the validated &str directly
            // rather than interpreting individual bytes as characters.
            let run_start = self.pos;
            while self.pos < self.input.len() {
                let b = self.input.as_bytes()[self.pos];
                if b == b'"' || b == b'\\' {
                    break;
                }
                self.pos += 1;
            }
            if self.pos > run_start {
                result.push_str(&self.input[run_start..self.pos]);
            }

            match self.next_byte() {
                Some(b'"') => return Ok(result),
                Some(b'\\') => {
                    match self.next_byte() {
                        Some(b'"') => result.push('"'),
                        Some(b'\\') => result.push('\\'),
                        Some(b'/') => result.push('/'),
                        Some(b'n') => result.push('\n'),
                        Some(b'r') => result.push('\r'),
                        Some(b't') => result.push('\t'),
                        Some(b'b') => result.push('\u{0008}'),
                        Some(b'f') => result.push('\u{000C}'),
                        Some(b'u') => {
                            let cp = self.parse_unicode_escape()?;
                            // Handle surrogate pairs
                            if (0xD800..=0xDBFF).contains(&cp) {
                                self.expect(b'\\')?;
                                self.expect(b'u')?;
                                let low = self.parse_unicode_escape()?;
                                if !(0xDC00..=0xDFFF).contains(&low) {
                                    return Err(self.error("invalid surrogate pair".into()));
                                }
                                let combined = 0x10000 + ((cp - 0xD800) << 10) + (low - 0xDC00);
                                result.push(
                                    char::from_u32(combined)
                                        .ok_or_else(|| self.error("invalid codepoint".into()))?,
                                );
                            } else {
                                result.push(
                                    char::from_u32(cp)
                                        .ok_or_else(|| self.error("invalid codepoint".into()))?,
                                );
                            }
                        }
                        Some(c) => {
                            return Err(self.error(format!("invalid escape '\\{}'", c as char)))
                        }
                        None => return Err(self.error("unterminated string escape".into())),
                    }
                }
                None => return Err(self.error("unterminated string".into())),
                _ => unreachable!("scan loop only stops at quote, backslash, or EOF"),
            }
        }
    }

    fn parse_unicode_escape(&mut self) -> Result<u32, ParseError> {
        let start = self.pos;
        if self.pos + 4 > self.input.len() {
            return Err(self.error("incomplete unicode escape".into()));
        }
        let hex = &self.input[start..start + 4];
        self.pos += 4;
        u32::from_str_radix(hex, 16)
            .map_err(|_| self.error(format!("invalid unicode escape: {hex}")))
    }

    fn parse_number(&mut self) -> Result<JsonValue, ParseError> {
        let start = self.pos;
        // Consume: optional minus
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        // Consume digits
        if self.peek() == Some(b'0') {
            self.pos += 1;
        } else {
            self.consume_digits()?;
        }
        // Fraction
        if self.peek() == Some(b'.') {
            self.pos += 1;
            self.consume_digits()?;
        }
        // Exponent
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            self.consume_digits()?;
        }
        let s = &self.input[start..self.pos];
        let n: f64 = s
            .parse()
            .map_err(|_| self.error(format!("invalid number: {s}")))?;
        Ok(JsonValue::Number(n))
    }

    fn consume_digits(&mut self) -> Result<(), ParseError> {
        let start = self.pos;
        while let Some(b) = self.peek() {
            if b.is_ascii_digit() {
                self.pos += 1;
            } else {
                break;
            }
        }
        if self.pos == start {
            Err(self.error("expected digit".into()))
        } else {
            Ok(())
        }
    }

    fn parse_object(&mut self) -> Result<JsonValue, ParseError> {
        self.expect(b'{')?;
        self.skip_whitespace();
        let mut map = BTreeMap::new();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(JsonValue::Object(map));
        }
        loop {
            self.skip_whitespace();
            let key = self.parse_string()?;
            self.skip_whitespace();
            self.expect(b':')?;
            let value = self.parse_value()?;
            map.insert(key, value);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                }
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(JsonValue::Object(map));
                }
                _ => return Err(self.error("expected ',' or '}' in object".into())),
            }
        }
    }

    fn parse_array(&mut self) -> Result<JsonValue, ParseError> {
        self.expect(b'[')?;
        self.skip_whitespace();
        let mut items = Vec::new();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(JsonValue::Array(items));
        }
        loop {
            let value = self.parse_value()?;
            items.push(value);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                }
                Some(b']') => {
                    self.pos += 1;
                    return Ok(JsonValue::Array(items));
                }
                _ => return Err(self.error("expected ',' or ']' in array".into())),
            }
        }
    }

    fn parse_literal(&mut self, expected: &str, value: JsonValue) -> Result<JsonValue, ParseError> {
        if self.input[self.pos..].starts_with(expected) {
            self.pos += expected.len();
            Ok(value)
        } else {
            Err(self.error(format!("expected '{expected}'")))
        }
    }

    // -- Helpers --

    fn peek(&self) -> Option<u8> {
        self.input.as_bytes().get(self.pos).copied()
    }

    fn next_byte(&mut self) -> Option<u8> {
        let b = self.input.as_bytes().get(self.pos).copied()?;
        self.pos += 1;
        Some(b)
    }

    fn expect(&mut self, expected: u8) -> Result<(), ParseError> {
        match self.next_byte() {
            Some(b) if b == expected => Ok(()),
            Some(b) => Err(self.error(format!(
                "expected '{}', found '{}'",
                expected as char, b as char
            ))),
            None => Err(self.error(format!("expected '{}', found EOF", expected as char))),
        }
    }

    fn skip_whitespace(&mut self) {
        while let Some(b) = self.peek() {
            if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn error(&self, msg: String) -> ParseError {
        ParseError(format!("at position {}: {msg}", self.pos))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_primitives() {
        assert_eq!(JsonValue::parse_str("null").unwrap(), JsonValue::Null);
        assert_eq!(JsonValue::parse_str("true").unwrap(), JsonValue::Bool(true));
        assert_eq!(
            JsonValue::parse_str("false").unwrap(),
            JsonValue::Bool(false)
        );
    }

    #[test]
    fn parse_numbers() {
        assert_eq!(JsonValue::parse_str("42").unwrap(), JsonValue::Number(42.0));
        assert_eq!(
            JsonValue::parse_str("-3.14").unwrap(),
            JsonValue::Number(-3.14)
        );
        assert_eq!(
            JsonValue::parse_str("1e10").unwrap(),
            JsonValue::Number(1e10)
        );
        assert_eq!(JsonValue::parse_str("0").unwrap(), JsonValue::Number(0.0));
    }

    #[test]
    fn parse_strings() {
        assert_eq!(
            JsonValue::parse_str(r#""hello""#).unwrap(),
            JsonValue::Str("hello".into())
        );
        assert_eq!(
            JsonValue::parse_str(r#""a\"b""#).unwrap(),
            JsonValue::Str("a\"b".into())
        );
        assert_eq!(
            JsonValue::parse_str(r#""a\\b""#).unwrap(),
            JsonValue::Str("a\\b".into())
        );
        assert_eq!(
            JsonValue::parse_str(r#""a\nb""#).unwrap(),
            JsonValue::Str("a\nb".into())
        );
        assert_eq!(
            JsonValue::parse_str(r#""\u0041""#).unwrap(),
            JsonValue::Str("A".into())
        );
    }

    #[test]
    fn parse_multibyte_utf8() {
        // This is the critical regression test for Bug 1 — multi-byte UTF-8
        // must survive a write→read round-trip without corruption.
        assert_eq!(
            JsonValue::parse_str(r#""café""#).unwrap(),
            JsonValue::Str("café".into())
        );
        assert_eq!(
            JsonValue::parse_str("\"日本語\"").unwrap(),
            JsonValue::Str("日本語".into())
        );
        assert_eq!(
            JsonValue::parse_str("\"emoji: 🦀\"").unwrap(),
            JsonValue::Str("emoji: 🦀".into())
        );
    }

    #[test]
    fn parse_array() {
        let val = JsonValue::parse_str("[1,2,3]").unwrap();
        let arr = val.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0].as_f64(), Some(1.0));
    }

    #[test]
    fn parse_object() {
        let val = JsonValue::parse_str(r#"{"a":1,"b":"two"}"#).unwrap();
        assert_eq!(val.get("a").unwrap().as_f64(), Some(1.0));
        assert_eq!(val.get("b").unwrap().as_str(), Some("two"));
    }

    #[test]
    fn parse_nested() {
        let val = JsonValue::parse_str(r#"{"key":[null,true,{"x":42}]}"#).unwrap();
        let arr = val.get("key").unwrap().as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert!(arr[0].is_null());
        assert_eq!(arr[1].as_bool(), Some(true));
        assert_eq!(arr[2].get("x").unwrap().as_f64(), Some(42.0));
    }

    #[test]
    fn parse_empty_containers() {
        assert_eq!(
            JsonValue::parse_str("{}").unwrap(),
            JsonValue::Object(BTreeMap::new())
        );
        assert_eq!(
            JsonValue::parse_str("[]").unwrap(),
            JsonValue::Array(vec![])
        );
    }

    #[test]
    fn parse_rejects_trailing_data() {
        assert!(JsonValue::parse_str("42 extra").is_err());
    }

    #[test]
    fn parse_whitespace() {
        let val = JsonValue::parse_str("  { \"a\" : 1 }  ").unwrap();
        assert_eq!(val.get("a").unwrap().as_f64(), Some(1.0));
    }

    #[test]
    fn roundtrip_serde_json_compat() {
        // Verify our parser handles the exact format serde_json produces for
        // the types Kronroe stores.
        let input = r#"{"UpsertFact":{"key":"alice:works_at:kf_01234","fact":{"id":"kf_01ARZ3NDEKTSV4RRFFQ69G5FAV","subject":"alice","predicate":"works_at","object":{"type":"Text","value":"Acme"},"valid_from":"2024-01-15T00:00:00Z","valid_to":null,"recorded_at":"2024-06-01T12:00:00Z","expired_at":null,"confidence":1,"source":null}}}"#;
        let val = JsonValue::parse_str(input).unwrap();
        assert!(val.get("UpsertFact").is_some());
        let inner = val.get("UpsertFact").unwrap();
        assert_eq!(
            inner.get("key").unwrap().as_str(),
            Some("alice:works_at:kf_01234")
        );
    }
}
