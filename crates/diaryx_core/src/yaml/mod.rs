//! YAML format primitives — pure parsing, serialization, and a dynamic value
//! type. Position-agnostic: operates on raw YAML strings with no knowledge
//! of frontmatter, endmatter, or markdown.
//!
//! For position-aware parsing (e.g. extracting YAML from between `---`
//! delimiters in a markdown file), see [`crate::frontmatter`].
//!
//! The backend is `fig` (a Zig-backed YAML parser/editor); this module is a
//! thin serde façade over `fig::from_str`/`fig::to_string`.

mod value;

pub use value::{Mapping, Value};

/// Errors produced by the underlying YAML parser/serializer.
pub type Error = fig::Error;

/// Deserialize a YAML string into a typed value.
pub fn from_str<T: serde::de::DeserializeOwned>(s: &str) -> Result<T, Error> {
    fig::from_str(s)
}

/// Parse a YAML string into the dynamic [`Value`] without serde.
///
/// The serde-free read path: parses with `fig`'s native parser and converts the
/// resulting tree, instead of routing through `from_str` / `Deserialize`. Use it
/// when the target is the dynamic `Value` (or a type that builds itself from
/// one), to keep serde out of the call graph. An empty document is [`Value::Null`].
pub fn parse_value(s: &str) -> Result<Value, Error> {
    let doc = fig::Document::parse(s.as_bytes(), fig::Format::Yaml)?;
    Ok(doc.to_value()?.into())
}

/// Serialize a value to a YAML string.
pub fn to_string<T: serde::Serialize + ?Sized>(value: &T) -> Result<String, Error> {
    fig::to_string(value)
}
