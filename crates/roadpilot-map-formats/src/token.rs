//! SCS tokens: short names packed into a `u64`.

use std::fmt;

/// Alphabet of token characters; index 0 marks the end of the name.
const ALPHABET: &[u8; 38] = b"\x000123456789abcdefghijklmnopqrstuvwxyz_";

/// A name of up to 12 characters from `0-9`, `a-z` and `_`, stored as a
/// base-38 number (first character in the lowest digit).
///
/// Verified on real sector files: every road look token of the inspected
/// ETS2 and ATS sectors decodes to the name of a `road_look` unit in the
/// game definitions, and every prefab model token to a `prefab_model` unit.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Token(pub u64);

impl Token {
    /// The decoded name, or `None` if the value is not a valid token (an
    /// end marker followed by more characters, or more than 12 characters).
    ///
    /// ```
    /// use roadpilot_map_formats::Token;
    ///
    /// // "ab" = 11 + 12 * 38
    /// assert_eq!(Token(11 + 12 * 38).name().as_deref(), Some("ab"));
    /// assert_eq!(Token(0).name().as_deref(), Some(""));
    /// ```
    #[must_use]
    pub fn name(self) -> Option<String> {
        let mut value = self.0;
        let mut out = String::new();
        while value > 0 {
            let digit = usize::try_from(value % 38).ok()?;
            let c = *ALPHABET.get(digit)?;
            if digit == 0 || out.len() == 12 {
                return None;
            }
            out.push(char::from(c));
            value /= 38;
        }
        Some(out)
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name() {
            Some(name) => write!(f, "Token({name:?})"),
            None => write!(f, "Token({:#018x})", self.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Token;

    fn encode(name: &str) -> u64 {
        name.bytes().rev().fold(0u64, |acc, c| {
            let digit = match c {
                b'0'..=b'9' => u64::from(c - b'0') + 1,
                b'a'..=b'z' => u64::from(c - b'a') + 11,
                _ => 37,
            };
            acc * 38 + digit
        })
    }

    #[test]
    fn decodes_names() {
        for name in ["a", "road_1", "x2_y", "abcdefghijkl", "_", "0"] {
            assert_eq!(Token(encode(name)).name().as_deref(), Some(name), "{name}");
        }
    }

    #[test]
    fn rejects_invalid_values() {
        // An end marker (digit 0) below a non-zero digit.
        assert_eq!(Token(38).name(), None);
        // More than 12 characters.
        assert_eq!(Token(u64::MAX).name(), None);
    }

    #[test]
    fn debug_shows_the_name() {
        assert_eq!(format!("{:?}", Token(encode("ab"))), "Token(\"ab\")");
    }
}
