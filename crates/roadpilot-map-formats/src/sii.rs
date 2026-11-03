//! Minimal reader for text SII definition files (`SiiNunit`).
//!
//! Only what the lane model needs is supported:
//!
//! - a unit is `class : name { ... }`, with `{` on the same or the next line,
//! - attributes are `key: value`, arrays are `key[]: value` (append) or
//!   `key[index]: value`; a plain `key: count` before indexed entries only
//!   declares the array size and is ignored,
//! - comments: `#`, `//` and `/* ... */`,
//! - `@include "file"` is resolved through a caller-supplied function, with
//!   a fixed depth limit and without recursion.
//!
//! Values are kept as text (quotes removed). Binary (`BSII`), obfuscated
//! (`3nK`) and encrypted (`ScsC`) SII files are rejected; none of them occur
//! in the definition files the lane model reads.

use std::collections::BTreeMap;
use std::fmt;

/// Largest accepted text, including all includes, in bytes.
pub const MAX_SII_BYTES: usize = 64 * 1024 * 1024;
/// Deepest accepted `@include` nesting.
pub const MAX_INCLUDE_DEPTH: usize = 8;
/// Largest accepted array index. Real definition arrays have a few dozen
/// entries at most.
pub const MAX_ARRAY_INDEX: usize = 65_535;

/// One unit of an SII file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SiiUnit {
    /// Unit class, for example `road_look`.
    pub class: String,
    /// Unit name, for example `road.look0`.
    pub name: String,
    /// Attribute values in file order. A scalar attribute has one value.
    pub attributes: BTreeMap<String, Vec<String>>,
}

impl SiiUnit {
    /// The single value of a scalar attribute.
    #[must_use]
    pub fn value(&self, key: &str) -> Option<&str> {
        self.attributes
            .get(key)
            .and_then(|v| v.first())
            .map(String::as_str)
    }

    /// All values of an array attribute (empty if absent).
    #[must_use]
    pub fn values(&self, key: &str) -> &[String] {
        self.attributes.get(key).map_or(&[], Vec::as_slice)
    }
}

/// Why an SII text could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SiiError {
    /// The text does not start with `SiiNunit` (after an optional UTF-8 BOM).
    NotTextSii,
    /// The text is not valid UTF-8.
    NotUtf8,
    /// The text, with includes, is larger than [`MAX_SII_BYTES`].
    TooLarge,
    /// An `@include` names a file that the resolver cannot provide.
    MissingInclude { path: String },
    /// Includes are nested deeper than [`MAX_INCLUDE_DEPTH`].
    IncludeTooDeep { path: String },
    /// A line could not be understood.
    Syntax { line: usize, reason: &'static str },
}

impl fmt::Display for SiiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotTextSii => f.write_str("not a text SII file (no SiiNunit header)"),
            Self::NotUtf8 => f.write_str("the SII text is not valid UTF-8"),
            Self::TooLarge => write!(f, "the SII text is larger than {MAX_SII_BYTES} bytes"),
            Self::MissingInclude { path } => write!(f, "included file {path:?} is missing"),
            Self::IncludeTooDeep { path } => {
                write!(
                    f,
                    "includes nested deeper than {MAX_INCLUDE_DEPTH} at {path:?}"
                )
            }
            Self::Syntax { line, reason } => write!(f, "SII syntax error on line {line}: {reason}"),
        }
    }
}

impl std::error::Error for SiiError {}

/// Parses a text SII file. `@include` lines are resolved with `include`,
/// which receives the path exactly as written and returns the file's bytes.
///
/// # Errors
///
/// A [`SiiError`] for non-text SII, invalid UTF-8, oversized input,
/// unresolvable or too deeply nested includes, and syntax errors.
pub fn parse_sii(
    bytes: &[u8],
    include: &mut dyn FnMut(&str) -> Option<Vec<u8>>,
) -> Result<Vec<SiiUnit>, SiiError> {
    let text = text_of(bytes)?;
    let body = text
        .trim_start()
        .strip_prefix("SiiNunit")
        .ok_or(SiiError::NotTextSii)?;
    let lines = expand_includes(body, include)?;
    parse_lines(&lines)
}

fn text_of(bytes: &[u8]) -> Result<&str, SiiError> {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(bytes);
    if bytes.len() > MAX_SII_BYTES {
        return Err(SiiError::TooLarge);
    }
    std::str::from_utf8(bytes).map_err(|_| SiiError::NotUtf8)
}

/// Replaces `@include` lines by the included lines, iteratively.
fn expand_includes(
    text: &str,
    include: &mut dyn FnMut(&str) -> Option<Vec<u8>>,
) -> Result<Vec<String>, SiiError> {
    // A stack of (remaining lines in reverse order, depth).
    let mut stack: Vec<(Vec<String>, usize)> =
        vec![(text.lines().rev().map(str::to_owned).collect(), 0)];
    let mut out = Vec::new();
    let mut total = text.len();
    while let Some((lines, depth)) = stack.last_mut() {
        let depth = *depth;
        let Some(line) = lines.pop() else {
            stack.pop();
            continue;
        };
        let Some(rest) = line.trim().strip_prefix("@include") else {
            out.push(line);
            continue;
        };
        let path = rest.trim().trim_matches('"').to_owned();
        if depth >= MAX_INCLUDE_DEPTH {
            return Err(SiiError::IncludeTooDeep { path });
        }
        let bytes =
            include(&path).ok_or_else(|| SiiError::MissingInclude { path: path.clone() })?;
        let text = text_of(&bytes)?;
        total = total.saturating_add(text.len());
        if total > MAX_SII_BYTES {
            return Err(SiiError::TooLarge);
        }
        stack.push((text.lines().rev().map(str::to_owned).collect(), depth + 1));
    }
    Ok(out)
}

/// Removes comments outside quoted strings. `in_block` carries an open
/// `/* */` comment across lines.
fn strip_comments(line: &str, in_block: &mut bool) -> String {
    let mut out = String::new();
    let mut chars = line.char_indices().peekable();
    let mut quoted = false;
    while let Some((i, c)) = chars.next() {
        if *in_block {
            if line[i..].starts_with("*/") {
                *in_block = false;
                chars.next();
            }
            continue;
        }
        if c == '"' {
            quoted = !quoted;
        } else if !quoted {
            if c == '#' || line[i..].starts_with("//") {
                break;
            }
            if line[i..].starts_with("/*") {
                *in_block = true;
                chars.next();
                continue;
            }
        }
        out.push(c);
    }
    out
}

fn parse_lines(lines: &[String]) -> Result<Vec<SiiUnit>, SiiError> {
    let mut units = Vec::new();
    let mut current: Option<SiiUnit> = None;
    let mut awaiting_brace = false;
    let mut in_block = false;
    let mut outer_open = false;
    // Keys whose only value so far is a number from a scalar line: possibly
    // an array size declaration (`key: 2` before `key[0]` or `key[]`).
    let mut size_declared: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (index, raw) in lines.iter().enumerate() {
        let line_no = index + 1;
        let syntax = |reason| SiiError::Syntax {
            line: line_no,
            reason,
        };
        let stripped = strip_comments(raw, &mut in_block);
        let line = stripped.trim();
        if line.is_empty() {
            continue;
        }
        if awaiting_brace {
            if line != "{" {
                return Err(syntax("expected '{' after the unit header"));
            }
            awaiting_brace = false;
            continue;
        }
        match current.as_mut() {
            None => {
                if line == "{" && !outer_open {
                    outer_open = true;
                    continue;
                }
                if line == "}" && outer_open {
                    outer_open = false;
                    continue;
                }
                let (header, opens) = match line.strip_suffix('{') {
                    Some(h) => (h.trim(), true),
                    None => (line, false),
                };
                let (class, name) = header
                    .split_once(" : ")
                    .ok_or(syntax("expected a unit header 'class : name'"))?;
                let (class, name) = (class.trim(), name.trim());
                if class.is_empty() || name.is_empty() || name.contains(char::is_whitespace) {
                    return Err(syntax("invalid unit header"));
                }
                size_declared.clear();
                current = Some(SiiUnit {
                    class: class.to_owned(),
                    name: name.to_owned(),
                    attributes: BTreeMap::new(),
                });
                awaiting_brace = !opens;
            }
            Some(unit) => {
                if line == "}" {
                    if let Some(done) = current.take() {
                        units.push(done);
                    }
                    continue;
                }
                let (key, value) = line
                    .split_once(':')
                    .ok_or(syntax("expected 'key: value'"))?;
                let (key, value) = (key.trim(), unquote(value.trim()));
                let array_base = key
                    .strip_suffix("[]")
                    .or_else(|| key.split_once('[').map(|(b, _)| b));
                if let Some(base) = array_base {
                    if size_declared.remove(base) {
                        unit.attributes.remove(base);
                    }
                }
                if let Some(base) = key.strip_suffix("[]") {
                    unit.attributes
                        .entry(base.to_owned())
                        .or_default()
                        .push(value);
                } else if let Some((base, rest)) = key.split_once('[') {
                    let idx: usize = rest
                        .strip_suffix(']')
                        .and_then(|i| i.parse().ok())
                        .ok_or(syntax("invalid array index"))?;
                    if idx > MAX_ARRAY_INDEX {
                        return Err(syntax("array index too large"));
                    }
                    let entry = unit.attributes.entry(base.to_owned()).or_default();
                    if entry.len() <= idx {
                        entry.resize(idx + 1, String::new());
                    }
                    entry[idx] = value;
                } else {
                    let entry = unit.attributes.entry(key.to_owned()).or_default();
                    if entry.is_empty() && value.parse::<u64>().is_ok() {
                        size_declared.insert(key.to_owned());
                    } else {
                        size_declared.remove(key);
                    }
                    entry.push(value);
                }
            }
        }
    }
    if current.is_some() || awaiting_brace {
        return Err(SiiError::Syntax {
            line: lines.len(),
            reason: "unit not closed",
        });
    }
    Ok(units)
}

fn unquote(value: &str) -> String {
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(value)
        .to_owned()
}

/// Parses a tuple value such as `(1.5, -2)` into its numbers.
#[must_use]
pub fn parse_tuple(value: &str) -> Option<Vec<f32>> {
    let inner = value.trim().strip_prefix('(')?.strip_suffix(')')?;
    inner
        .split(',')
        .map(|p| p.trim().parse::<f32>().ok().filter(|v| v.is_finite()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_includes(_: &str) -> Option<Vec<u8>> {
        None
    }

    fn parse(text: &str) -> Result<Vec<SiiUnit>, SiiError> {
        parse_sii(text.as_bytes(), &mut no_includes)
    }

    #[test]
    fn parses_units_scalars_and_arrays() {
        let units = parse(
            "SiiNunit\n{\nroad_look : road.a\n{\n name: \"A b\"\n lanes_right[]: t.x\n lanes_right[]: t.y\n size: 4.5\n}\nother : o.b {\n list: 2\n list[0]: p\n list[1]: q\n}\n}\n",
        )
        .expect("valid");
        assert_eq!(units.len(), 2);
        assert_eq!(units[0].class, "road_look");
        assert_eq!(units[0].name, "road.a");
        assert_eq!(units[0].value("name"), Some("A b"));
        assert_eq!(units[0].values("lanes_right"), ["t.x", "t.y"]);
        assert_eq!(units[0].value("size"), Some("4.5"));
        assert_eq!(units[1].values("list"), ["p", "q"]);
        // A size declaration before appends is dropped too; a plain number
        // that is never used as an array stays a scalar.
        let declared = parse(
            "SiiNunit
{
u : n.a {
 a: 2
 a[]: x
 a[]: y
 n: 7
}
}
",
        )
        .expect("valid");
        assert_eq!(declared[0].values("a"), ["x", "y"]);
        assert_eq!(declared[0].value("n"), Some("7"));
        assert!(units[1].values("missing").is_empty());
    }

    #[test]
    fn strips_comments_but_not_inside_strings() {
        let units = parse(
            "SiiNunit\n{\n# comment\nu : n.a { // trailing\n s: \"a#b//c\" # x\n /* block\n k: lost\n */ k2: kept\n}\n}\n",
        )
        .expect("valid");
        assert_eq!(units[0].value("s"), Some("a#b//c"));
        assert_eq!(units[0].value("k"), None);
        assert_eq!(units[0].value("k2"), Some("kept"));
    }

    #[test]
    fn resolves_includes_with_a_depth_limit() {
        let mut files = |p: &str| match p {
            "a.sui" => Some(b"u : n.a {\n@include \"b.sui\"\n}\n".to_vec()),
            "b.sui" => Some(b" k: v\n".to_vec()),
            "loop.sui" => Some(b"@include \"loop.sui\"\n".to_vec()),
            _ => None,
        };
        let units = parse_sii(b"SiiNunit\n{\n@include \"a.sui\"\n}\n", &mut files).expect("valid");
        assert_eq!(units[0].value("k"), Some("v"));
        assert!(matches!(
            parse_sii(b"SiiNunit\n{\n@include \"loop.sui\"\n}\n", &mut files),
            Err(SiiError::IncludeTooDeep { .. })
        ));
        assert!(matches!(
            parse_sii(b"SiiNunit\n{\n@include \"none.sui\"\n}\n", &mut files),
            Err(SiiError::MissingInclude { .. })
        ));
    }

    #[test]
    fn accepts_a_bom() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(b"SiiNunit\n{\nu : n.a {\n}\n}\n");
        assert_eq!(parse_sii(&bytes, &mut no_includes).map(|u| u.len()), Ok(1));
    }

    #[test]
    fn rejects_malformed_input() {
        for (text, what) in [
            ("BSII....", "binary"),
            ("SiiNunit\n{\nu : n.a {\n k v\n}\n}\n", "no colon"),
            ("SiiNunit\n{\nu : n.a {\n k: v\n", "unclosed"),
            ("SiiNunit\n{\nnot a header\n}\n", "header"),
            ("SiiNunit\n{\nu : n.a\n k: v\n}\n", "missing brace"),
            ("SiiNunit\n{\nu : n.a {\n k[x]: v\n}\n}\n", "bad index"),
            (
                "SiiNunit\n{\nu : n.a {\n k[999999999]: v\n}\n}\n",
                "huge index",
            ),
        ] {
            assert!(parse(text).is_err(), "{what}");
        }
        assert_eq!(
            parse_sii(&[0xFF, 0xFE], &mut no_includes),
            Err(SiiError::NotUtf8)
        );
    }

    #[test]
    fn parses_tuples() {
        assert_eq!(parse_tuple("(1.5, -2)"), Some(vec![1.5, -2.0]));
        assert_eq!(parse_tuple("(1, x)"), None);
        assert_eq!(parse_tuple("1, 2"), None);
        assert_eq!(parse_tuple("(nan, 1)"), None);
    }
}
