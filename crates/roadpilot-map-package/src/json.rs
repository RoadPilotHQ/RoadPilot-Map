//! A small JSON reader and writer helpers for the line formats of
//! RoadPilot-Map (the navigation graph export and the map package).
//!
//! The reader accepts standard JSON for one value, with a depth limit. It is
//! an internal helper that is public so that the facade crate can share it;
//! it is not a general-purpose JSON library.

use std::fmt::Write as _;

/// Maximum nesting depth the reader accepts.
const MAX_DEPTH: usize = 16;

/// A parsed JSON value. Objects keep their member order.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Self::Obj(members) => members.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Int(i) => Some(*i),
            _ => None,
        }
    }

    /// An integer or float as `f64`.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Int(i) => Some(*i as f64),
            Self::Float(f) => Some(*f),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Self::Arr(a) => Some(a),
            _ => None,
        }
    }
}

/// Parses one JSON value that fills the whole input (surrounding white space
/// is allowed).
pub fn parse(input: &str) -> Result<Json, String> {
    let mut p = Parser {
        bytes: input.as_bytes(),
        pos: 0,
    };
    let value = p.value(0)?;
    p.skip_ws();
    if p.pos != p.bytes.len() {
        return Err(format!("unexpected data at byte {}", p.pos));
    }
    Ok(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn skip_ws(&mut self) {
        while matches!(self.bytes.get(self.pos), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn expect(&mut self, b: u8) -> Result<(), String> {
        if self.peek() == Some(b) {
            self.pos += 1;
            Ok(())
        } else {
            Err(format!("expected '{}' at byte {}", b as char, self.pos))
        }
    }

    fn literal(&mut self, word: &str, value: Json) -> Result<Json, String> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(value)
        } else {
            Err(format!("invalid literal at byte {}", self.pos))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, String> {
        if depth > MAX_DEPTH {
            return Err("nesting is too deep".to_owned());
        }
        self.skip_ws();
        match self.peek() {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => self.string().map(Json::Str),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'n') => self.literal("null", Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(format!("unexpected input at byte {}", self.pos)),
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, String> {
        self.expect(b'{')?;
        let mut members = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Json::Obj(members));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            self.expect(b':')?;
            let value = self.value(depth + 1)?;
            members.push((key, value));
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Json::Obj(members));
                }
                _ => return Err(format!("expected ',' or '}}' at byte {}", self.pos)),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, String> {
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Json::Arr(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Json::Arr(items));
                }
                _ => return Err(format!("expected ',' or ']' at byte {}", self.pos)),
            }
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        let mut float = false;
        while let Some(b) = self.peek() {
            match b {
                b'0'..=b'9' => {}
                b'.' | b'e' | b'E' | b'+' | b'-' => float = true,
                _ => break,
            }
            self.pos += 1;
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| "invalid number".to_owned())?;
        if !float {
            if let Ok(i) = text.parse::<i64>() {
                return Ok(Json::Int(i));
            }
        }
        text.parse::<f64>()
            .ok()
            .filter(|f| f.is_finite())
            .map(Json::Float)
            .ok_or_else(|| format!("invalid number at byte {start}"))
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let digits = self
            .bytes
            .get(self.pos..self.pos + 4)
            .ok_or_else(|| "truncated \\u escape".to_owned())?;
        let text = std::str::from_utf8(digits).map_err(|_| "invalid \\u escape".to_owned())?;
        let v = u32::from_str_radix(text, 16).map_err(|_| "invalid \\u escape".to_owned())?;
        self.pos += 4;
        Ok(v)
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            // Copy a run of plain bytes at once.
            let start = self.pos;
            while let Some(b) = self.peek() {
                if b == b'"' || b == b'\\' || b < 0x20 {
                    break;
                }
                self.pos += 1;
            }
            out.push_str(
                std::str::from_utf8(&self.bytes[start..self.pos])
                    .map_err(|_| "invalid UTF-8 in string".to_owned())?,
            );
            match self.peek() {
                Some(b'"') => {
                    self.pos += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.pos += 1;
                    let esc = self.peek().ok_or_else(|| "truncated escape".to_owned())?;
                    self.pos += 1;
                    match esc {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let first = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&first) {
                                if self.bytes.get(self.pos..self.pos + 2) != Some(b"\\u") {
                                    return Err("unpaired surrogate".to_owned());
                                }
                                self.pos += 2;
                                let second = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&second) {
                                    return Err("unpaired surrogate".to_owned());
                                }
                                0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
                            } else {
                                first
                            };
                            out.push(
                                char::from_u32(code)
                                    .ok_or_else(|| "invalid code point".to_owned())?,
                            );
                        }
                        _ => return Err("invalid escape".to_owned()),
                    }
                }
                _ => return Err("unterminated string".to_owned()),
            }
        }
    }
}

/// Appends `s` as a JSON string, escaped.
pub fn push_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Appends a finite `f64` in its shortest round-trip decimal form. The same
/// value always gives the same text. Returns `false` and writes `null` for a
/// value that is not finite (the caller must treat that as an error).
pub fn push_f64(out: &mut String, v: f64) -> bool {
    if v.is_finite() {
        // `Display` for f64 is the shortest text that reads back exactly and
        // never uses an exponent, so it is valid JSON. `-0` is normalized.
        let v = if v == 0.0 { 0.0 } else { v };
        let _ = write!(out, "{v}");
        true
    } else {
        out.push_str("null");
        false
    }
}

/// Appends a 64-bit identifier as a lower-case hexadecimal JSON string.
pub fn push_hex(out: &mut String, v: u64) {
    let _ = write!(out, "\"{v:x}\"");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scalars_arrays_and_objects() {
        let v = parse(r#" {"a":1,"b":[true,null,-2],"c":"x","d":1.5} "#).expect("valid");
        assert_eq!(v.get("a").and_then(Json::as_i64), Some(1));
        assert_eq!(v.get("c").and_then(Json::as_str), Some("x"));
        assert_eq!(v.get("d"), Some(&Json::Float(1.5)));
        let b = v.get("b").and_then(Json::as_array).expect("array");
        assert_eq!(b, [Json::Bool(true), Json::Null, Json::Int(-2)]);
        assert_eq!(v.get("missing"), None);
    }

    #[test]
    fn handles_escapes_and_surrogate_pairs() {
        let v = parse(r#""a\"b\\c\nA😀""#).expect("valid");
        assert_eq!(v.as_str(), Some("a\"b\\c\nA\u{1F600}"));
    }

    #[test]
    fn rejects_invalid_input_without_panicking() {
        for bad in [
            "",
            "{",
            "[1,]",
            "{\"a\"}",
            "tru",
            "\"abc",
            r#""\x""#,
            r#""\ud800""#,
            "1 2",
            "[1}",
            "{\"a\":1,}",
            "-",
            r#""\u12""#,
            "\u{1}",
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn limits_the_nesting_depth() {
        let deep = format!("{}1{}", "[".repeat(40), "]".repeat(40));
        assert!(parse(&deep).is_err());
        let ok = format!("{}1{}", "[".repeat(10), "]".repeat(10));
        assert!(parse(&ok).is_ok());
    }

    #[test]
    fn written_strings_read_back_unchanged() {
        for s in [
            "plain",
            "quote\" back\\slash",
            "tab\tnew\nline",
            "\u{1}control",
            "\u{fc}n\u{ef}\u{1F600}",
        ] {
            let mut out = String::new();
            push_str(&mut out, s);
            assert_eq!(parse(&out).expect("valid").as_str(), Some(s));
        }
        let mut out = String::new();
        push_hex(&mut out, 0x2524_9798_d200_0001);
        assert_eq!(out, "\"25249798d2000001\"");
    }
}
