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

/// Serialize a value to a YAML string.
pub fn to_string<T: serde::Serialize + ?Sized>(value: &T) -> Result<String, Error> {
    fig::to_string(value)
}
