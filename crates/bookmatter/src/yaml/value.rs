//! Dynamic YAML value type with order-preserving mappings.
//!
//! Replaces `serde_yaml_ng::Value` with a lightweight enum that uses String keys
//! in mappings (rather than `serde_yaml_ng::Value` keys) and separates integers
//! from floats. Uses hand-written Serialize/Deserialize impls to keep code
//! generation minimal.

use indexmap::IndexMap;
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A YAML value type for frontmatter and configuration data.
///
/// Maps to `JsonValue` in TypeScript bindings (same representation as
/// `serde_json::Value`).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "typescript",
    ts(
        export,
        export_to = "bindings/",
        rename = "YamlValue",
        type = "null | boolean | number | string | YamlValue[] | { [key: string]: YamlValue }"
    )
)]
pub enum Value {
    /// YAML null (`~`, `null`)
    Null,
    /// YAML boolean
    Bool(bool),
    /// YAML integer
    Int(i64),
    /// YAML float
    Float(f64),
    /// YAML string
    String(String),
    /// YAML sequence (`- item`)
    Sequence(Vec<Value>),
    /// YAML mapping (`key: value`) with ordered string keys
    Mapping(IndexMap<String, Value>),
}

/// Type alias for YAML mappings (preserves key ordering).
pub type Mapping = IndexMap<String, Value>;

// ============================================================================
// Serialize
// ============================================================================

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Value::Null => serializer.serialize_none(),
            Value::Bool(b) => serializer.serialize_bool(*b),
            Value::Int(i) => serializer.serialize_i64(*i),
            Value::Float(f) => serializer.serialize_f64(*f),
            Value::String(s) => serializer.serialize_str(s),
            Value::Sequence(seq) => {
                let mut s = serializer.serialize_seq(Some(seq.len()))?;
                for item in seq {
                    s.serialize_element(item)?;
                }
                s.end()
            }
            Value::Mapping(map) => {
                let mut m = serializer.serialize_map(Some(map.len()))?;
                for (k, v) in map {
                    m.serialize_entry(k, v)?;
                }
                m.end()
            }
        }
    }
}

// ============================================================================
// Deserialize
// ============================================================================

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ValueVisitor)
    }
}

struct ValueVisitor;

impl<'de> Visitor<'de> for ValueVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("any YAML value")
    }

    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        Deserialize::deserialize(deserializer)
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Value, E> {
        Ok(Value::Int(v))
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Value, E> {
        if v <= i64::MAX as u64 {
            Ok(Value::Int(v as i64))
        } else {
            Ok(Value::Float(v as f64))
        }
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Value, E> {
        Ok(Value::Float(v))
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Value, E> {
        Ok(Value::String(v.to_owned()))
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<Value, E> {
        Ok(Value::String(v))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut values = Vec::with_capacity(seq.size_hint().unwrap_or(0));
        while let Some(elem) = seq.next_element()? {
            values.push(elem);
        }
        Ok(Value::Sequence(values))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut values = IndexMap::with_capacity(map.size_hint().unwrap_or(0));
        while let Some((key, value)) = map.next_entry::<String, Value>()? {
            values.insert(key, value);
        }
        Ok(Value::Mapping(values))
    }
}

// ============================================================================
// Convenience methods
// ============================================================================

impl Value {
    /// Returns the string value, if this is a `String`.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    /// Returns the boolean value, if this is a `Bool`.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Returns the value as `i64`, if this is an `Int` or a lossless `Float`.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            Value::Float(f) => {
                let i = *f as i64;
                if (i as f64) == *f { Some(i) } else { None }
            }
            _ => None,
        }
    }

    /// Returns the value as `u64`, if this is a non-negative `Int`.
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Value::Int(i) if *i >= 0 => Some(*i as u64),
            _ => None,
        }
    }

    /// Returns the value as `f64`, if this is a `Float` or `Int`.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Float(f) => Some(*f),
            Value::Int(i) => Some(*i as f64),
            _ => None,
        }
    }

    /// Returns the sequence, if this is a `Sequence`.
    pub fn as_sequence(&self) -> Option<&Vec<Value>> {
        match self {
            Value::Sequence(v) => Some(v),
            _ => None,
        }
    }

    /// Returns a mutable reference to the sequence, if this is a `Sequence`.
    pub fn as_sequence_mut(&mut self) -> Option<&mut Vec<Value>> {
        match self {
            Value::Sequence(v) => Some(v),
            _ => None,
        }
    }

    /// Returns the mapping, if this is a `Mapping`.
    pub fn as_mapping(&self) -> Option<&IndexMap<String, Value>> {
        match self {
            Value::Mapping(m) => Some(m),
            _ => None,
        }
    }

    /// Returns a mutable reference to the mapping, if this is a `Mapping`.
    pub fn as_mapping_mut(&mut self) -> Option<&mut IndexMap<String, Value>> {
        match self {
            Value::Mapping(m) => Some(m),
            _ => None,
        }
    }

    /// Returns `true` if this value is `Null`.
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Returns `true` if this value is a `String`.
    pub fn is_string(&self) -> bool {
        matches!(self, Value::String(_))
    }

    /// Returns `true` if this value is a `Sequence`.
    pub fn is_sequence(&self) -> bool {
        matches!(self, Value::Sequence(_))
    }

    /// Returns `true` if this value is a `Mapping`.
    pub fn is_mapping(&self) -> bool {
        matches!(self, Value::Mapping(_))
    }

    /// Returns `true` if this value is a number (`Int` or `Float`).
    pub fn is_number(&self) -> bool {
        matches!(self, Value::Int(_) | Value::Float(_))
    }

    /// Access a mapping value by key.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_mapping().and_then(|m| m.get(key))
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Null => write!(f, "null"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Int(i) => write!(f, "{i}"),
            // Format floats via ryu (already a transitive dep of serde_json)
            // rather than the stdlib `Display` impl. Avoids pulling
            // `core::num::flt2dec` (the Dragon4/Grisu float formatter,
            // ~15 KB of WASM code) into the binary.
            Value::Float(v) => {
                let mut buf = ryu::Buffer::new();
                f.write_str(buf.format(*v))
            }
            Value::String(s) => write!(f, "{s}"),
            Value::Sequence(_) | Value::Mapping(_) => match serde_yaml_ng::to_string(self) {
                Ok(s) => write!(f, "{}", s.trim()),
                Err(_) => write!(f, "<complex value>"),
            },
        }
    }
}

// ============================================================================
// Conversions
// ============================================================================

#[cfg(feature = "json")]
impl From<serde_json::Value> for Value {
    fn from(json: serde_json::Value) -> Self {
        match json {
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Bool(b) => Value::Bool(b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Value::Int(i)
                } else if let Some(f) = n.as_f64() {
                    Value::Float(f)
                } else {
                    Value::Null
                }
            }
            serde_json::Value::String(s) => Value::String(s),
            serde_json::Value::Array(arr) => {
                Value::Sequence(arr.into_iter().map(Value::from).collect())
            }
            serde_json::Value::Object(map) => {
                let m: IndexMap<String, Value> =
                    map.into_iter().map(|(k, v)| (k, Value::from(v))).collect();
                Value::Mapping(m)
            }
        }
    }
}

#[cfg(feature = "json")]
impl From<Value> for serde_json::Value {
    fn from(yaml: Value) -> Self {
        match yaml {
            Value::Null => serde_json::Value::Null,
            Value::Bool(b) => serde_json::Value::Bool(b),
            Value::Int(i) => serde_json::Value::Number(i.into()),
            Value::Float(f) => serde_json::Number::from_f64(f)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
            Value::String(s) => serde_json::Value::String(s),
            Value::Sequence(arr) => {
                serde_json::Value::Array(arr.into_iter().map(serde_json::Value::from).collect())
            }
            Value::Mapping(map) => {
                let obj: serde_json::Map<String, serde_json::Value> = map
                    .into_iter()
                    .map(|(k, v)| (k, serde_json::Value::from(v)))
                    .collect();
                serde_json::Value::Object(obj)
            }
        }
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::String(s.to_string())
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::String(s)
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}

impl From<i64> for Value {
    fn from(i: i64) -> Self {
        Value::Int(i)
    }
}

impl From<f64> for Value {
    fn from(f: f64) -> Self {
        Value::Float(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_yaml() {
        let yaml = "title: Hello\ncount: 42\ntags:\n- a\n- b\n";
        let value: Value = serde_yaml_ng::from_str(yaml).unwrap();

        assert!(value.is_mapping());
        let map = value.as_mapping().unwrap();
        assert_eq!(map.get("title").unwrap().as_str(), Some("Hello"));
        assert_eq!(map.get("count").unwrap().as_i64(), Some(42));
        assert!(map.get("tags").unwrap().is_sequence());
    }

    #[cfg(feature = "json")]
    #[test]
    fn json_round_trip() {
        let yaml_val = Value::Mapping(IndexMap::from([
            ("key".to_string(), Value::String("value".to_string())),
            ("num".to_string(), Value::Int(42)),
        ]));

        let json: serde_json::Value = yaml_val.clone().into();
        let back: Value = json.into();
        assert_eq!(yaml_val, back);
    }
}
