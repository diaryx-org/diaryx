//! YAML format primitives — pure parsing, serialization, and a dynamic value
//! type. Position-agnostic: operates on raw YAML strings with no knowledge
//! of frontmatter, endmatter, or markdown.
//!
//! For position-aware parsing (e.g. extracting YAML from between `---`
//! delimiters in a markdown file), see [`crate::frontmatter`].
//!
//! The backend is `fig` (a Zig-backed YAML parser/editor). The dynamic-value
//! entry points ([`parse_value`], [`parse_mapping`], [`serialize_mapping`]) are
//! serde-free — they use `fig`'s native value tree — so they keep serde out of
//! the call graph (and out of the wasm binary). The generic [`from_str`] /
//! [`to_string`] remain a serde façade for callers deserializing into typed
//! structs (CLI config, plugins).

mod value;

pub use value::{Mapping, Value};

/// Errors produced by the underlying YAML parser/serializer.
pub type Error = fig::Error;

/// Deserialize a YAML string into a typed value via serde.
///
/// Prefer the serde-free [`parse_value`] / [`parse_mapping`] when the target is
/// the dynamic [`Value`] — this generic form pulls `fig`'s serde deserializer
/// into the binary and should be reserved for typed structs.
pub fn from_str<T: serde::de::DeserializeOwned>(s: &str) -> Result<T, Error> {
    fig::from_str(s)
}

/// Serialize a value to a YAML string via serde.
///
/// Prefer the serde-free [`serialize_mapping`] for dynamic mappings; this
/// generic form is for typed structs.
pub fn to_string<T: serde::Serialize + ?Sized>(value: &T) -> Result<String, Error> {
    fig::to_string(value)
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

/// Parse a JSON string into the dynamic [`Value`] without serde_json.
///
/// The serde_json-free read path: parses with `fig`'s JSON parser and converts
/// the resulting tree. Use it where JSON text (HTTP bodies, JS-supplied params,
/// stored config) was previously read via `serde_json::from_str::<Value>`.
pub fn parse_json(s: &str) -> Result<Value, Error> {
    let doc = fig::Document::parse(s.as_bytes(), fig::Format::Json)?;
    Ok(doc.to_value()?.into())
}

/// Parse a YAML mapping — the shape of frontmatter — into an ordered map,
/// serde-free. An empty document yields an empty map; a non-mapping top level is
/// a parse error (frontmatter must be a mapping). The serde-free replacement for
/// `from_str::<IndexMap<String, Value>>`.
pub fn parse_mapping(s: &str) -> Result<Mapping, Error> {
    match parse_value(s)? {
        Value::Mapping(map) => Ok(map),
        Value::Null => Ok(Mapping::new()),
        _ => Err(Error::expected_mapping("frontmatter mapping")),
    }
}

/// Serialize an ordered YAML mapping to a string without serde, by building
/// `fig`'s native value tree directly and rendering it with fig's serializer.
/// The serde-free replacement for `to_string(&IndexMap<String, Value>)`; output
/// is identical (same fig core serializer, same value tree).
pub fn serialize_mapping(map: &Mapping) -> Result<String, Error> {
    let value = fig::Value::Map(
        map.iter()
            .map(|(k, v)| (fig::Value::Str(k.clone()), fig::Value::from(v)))
            .collect(),
    );
    value.serialize(fig::Format::Yaml)
}
