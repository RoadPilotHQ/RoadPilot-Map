//! Reader for the text material files (`.mat`) that road template
//! descriptors reference.
//!
//! Only three things are read: the **effect** name, the top-level
//! **`substance`** attribute, and the **`source`** of each `texture` block.
//! Everything else is kept as raw text attributes and not interpreted.
//!
//! The form was established on every material referenced by the road
//! templates of local ETS2 and ATS installations:
//!
//! ```text
//! effect : "<effect name>" {
//!     <key> : <value>
//!     texture : "<slot>" {
//!         source : "<texture path>"
//!         <key> : <value>
//!     }
//! }
//! ```
//!
//! What the effect, substance or texture names *mean* is not established
//! here; see `docs/formats/scs-road-templates.md`.

use std::fmt;

/// Largest material file accepted, bytes.
pub const MAX_MAT_LEN: usize = 64 << 10;

/// Why a `.mat` file was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MatError {
    /// The file is larger than [`MAX_MAT_LEN`].
    TooLarge,
    /// The file is not UTF-8 text (binary materials are not supported).
    NotText,
    /// The text does not have the expected form; `line` counts from 1.
    Syntax { line: usize, what: &'static str },
}

impl fmt::Display for MatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => f.write_str("the material file is too large"),
            Self::NotText => f.write_str("the material file is not UTF-8 text"),
            Self::Syntax { line, what } => write!(f, "material line {line}: {what}"),
        }
    }
}

impl std::error::Error for MatError {}

/// A texture slot of a material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatTexture {
    /// The slot name, for example `texture_base`.
    pub slot: String,
    /// The texture path, if the block has a `source`.
    pub source: Option<String>,
}

/// The parts of a material that the reader extracts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Material {
    /// The effect name, for example `eut2.dif.spec.rfx`.
    pub effect: String,
    /// The top-level `substance` attribute, if present.
    pub substance: Option<String>,
    pub textures: Vec<MatTexture>,
    /// Other top-level attributes as raw `(key, value)` text, in file order.
    pub attributes: Vec<(String, String)>,
}

fn unquote(s: &str) -> Option<&str> {
    let s = s.trim();
    s.strip_prefix('"')?.strip_suffix('"')
}

/// Splits `key : value`. The key must be a plain identifier.
fn key_value(line: &str) -> Option<(&str, &str)> {
    let (k, v) = line.split_once(':')?;
    let k = k.trim();
    if k.is_empty() || !k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return None;
    }
    Some((k, v.trim()))
}

/// Parses a `.mat` text file.
///
/// # Errors
///
/// [`MatError`] for files that are too large, not text, or not of the form
/// shown in the module documentation (for example unbalanced braces, nesting
/// deeper than two levels, or a missing effect line).
pub fn parse_mat(bytes: &[u8]) -> Result<Material, MatError> {
    if bytes.len() > MAX_MAT_LEN {
        return Err(MatError::TooLarge);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| MatError::NotText)?;
    let mut effect: Option<String> = None;
    let mut substance = None;
    let mut textures: Vec<MatTexture> = Vec::new();
    let mut attributes = Vec::new();
    let mut depth = 0usize;
    let mut closed = false;
    for (i, raw) in text.lines().enumerate() {
        let line_no = i + 1;
        let err = |what| MatError::Syntax {
            line: line_no,
            what,
        };
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }
        if closed {
            return Err(err("text after the closing brace"));
        }
        if line == "}" {
            match depth {
                0 => return Err(err("unbalanced closing brace")),
                1 => closed = true,
                _ => {}
            }
            depth -= 1;
            continue;
        }
        if let Some(head) = line.strip_suffix('{') {
            let (k, v) =
                key_value(head).ok_or(err("a block must start with `key : \"name\" {`"))?;
            let name = unquote(v).ok_or(err("a block name must be quoted"))?;
            match (depth, k) {
                (0, "effect") => effect = Some(name.to_owned()),
                (0, _) => return Err(err("the file must start with an effect block")),
                (1, "texture") => textures.push(MatTexture {
                    slot: name.to_owned(),
                    source: None,
                }),
                (1, _) => return Err(err("only texture blocks may be nested")),
                _ => return Err(err("blocks are nested too deeply")),
            }
            depth += 1;
            continue;
        }
        let (k, v) = key_value(line).ok_or(err("expected `key : value`"))?;
        match depth {
            0 => return Err(err("an attribute outside the effect block")),
            1 => {
                if k == "substance" {
                    substance = Some(
                        unquote(v)
                            .ok_or(err("substance must be quoted"))?
                            .to_owned(),
                    );
                } else {
                    attributes.push((k.to_owned(), v.to_owned()));
                }
            }
            _ => {
                if k == "source" {
                    let source = unquote(v).ok_or(err("a texture source must be quoted"))?;
                    if let Some(t) = textures.last_mut() {
                        t.source = Some(source.to_owned());
                    }
                }
            }
        }
    }
    if !closed {
        return Err(MatError::Syntax {
            line: text.lines().count(),
            what: "the effect block is not closed",
        });
    }
    Ok(Material {
        effect: effect.ok_or(MatError::Syntax {
            line: 1,
            what: "no effect block",
        })?,
        substance,
        textures,
        attributes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Invented material, not game data.
    const SAMPLE: &str = "effect : \"test.dif.decal.rfx\" {\n\tshininess : 4.0\n\tdiffuse : { 1.0 , 1.0 , 1.0 }\n\tsubstance : \"gravel\"\n\ttexture : \"texture_base\" {\n\t\tsource : \"/material/test/a.tobj\"\n\t\tsampler : default\n\t}\n\ttexture : \"texture_mult\" {\n\t\tsampler : default\n\t}\n}\n";

    #[test]
    fn reads_effect_substance_and_textures() {
        let m = parse_mat(SAMPLE.as_bytes()).expect("valid");
        assert_eq!(m.effect, "test.dif.decal.rfx");
        assert_eq!(m.substance.as_deref(), Some("gravel"));
        assert_eq!(m.textures.len(), 2);
        assert_eq!(
            m.textures[0].source.as_deref(),
            Some("/material/test/a.tobj")
        );
        assert_eq!(m.textures[1].source, None);
        assert_eq!(
            m.attributes[1],
            ("diffuse".to_owned(), "{ 1.0 , 1.0 , 1.0 }".to_owned())
        );
    }

    #[test]
    fn substance_is_optional() {
        let m = parse_mat(b"effect : \"e.rfx\" {\n\tqueue_bias : 0\n}").expect("valid");
        assert_eq!(m.substance, None);
        assert!(m.textures.is_empty());
    }

    #[test]
    fn rejects_malformed_text() {
        for bad in [
            "",
            "material : \"old\" {\n}",
            "effect : \"e\" {\n",
            "effect : \"e\" {\n}\n}",
            "effect : \"e\" {\n}\nx : 1",
            "effect : e {\n}",
            "effect : \"e\" {\n\tsubstance : road\n}",
            "effect : \"e\" {\n\tnot a pair\n}",
            "effect : \"e\" {\n\tother : \"x\" {\n\t}\n}",
            "effect : \"e\" {\n\ttexture : \"t\" {\n\t\tinner : \"x\" {\n\t\t}\n\t}\n}",
            "x : 1\neffect : \"e\" {\n}",
        ] {
            assert!(parse_mat(bad.as_bytes()).is_err(), "{bad:?}");
        }
        assert_eq!(parse_mat(&[0xff, 0xfe]), Err(MatError::NotText));
        assert_eq!(
            parse_mat(&vec![b' '; MAX_MAT_LEN + 1]),
            Err(MatError::TooLarge)
        );
    }
}
