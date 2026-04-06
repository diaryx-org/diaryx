//! A minimal YAML value type for dynamic frontmatter manipulation.
//!
//! Replaces `serde_yaml::Value` with a lightweight enum that uses String keys
//! in mappings (rather than `serde_yaml::Value` keys) and separates integers
//! from floats. Uses hand-written Serialize/Deserialize impls to keep code
//! generation minimal.

use indexmap::IndexMap;
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A YAML value type for frontmatter and configuration data.
#[derive(Debug, Clone, PartialEq)]
pub enum YamlValue {
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
    Sequence(Vec<YamlValue>),
    /// YAML mapping (`key: value`) with ordered string keys
    Mapping(IndexMap<String, YamlValue>),
}

/// Type alias for YAML mappings (preserves key ordering).
pub type YamlMapping = IndexMap<String, YamlValue>;

// ============================================================================
// Serialize
// ============================================================================

impl Serialize for YamlValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            YamlValue::Null => serializer.serialize_none(),
            YamlValue::Bool(b) => serializer.serialize_bool(*b),
            YamlValue::Int(i) => serializer.serialize_i64(*i),
            YamlValue::Float(f) => serializer.serialize_f64(*f),
            YamlValue::String(s) => serializer.serialize_str(s),
            YamlValue::Sequence(seq) => {
                let mut s = serializer.serialize_seq(Some(seq.len()))?;
                for item in seq {
                    s.serialize_element(item)?;
                }
                s.end()
            }
            YamlValue::Mapping(map) => {
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

impl<'de> Deserialize<'de> for YamlValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(YamlValueVisitor)
    }
}

struct YamlValueVisitor;

impl<'de> Visitor<'de> for YamlValueVisitor {
    type Value = YamlValue;

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("any YAML value")
    }

    fn visit_unit<E: de::Error>(self) -> Result<YamlValue, E> {
        Ok(YamlValue::Null)
    }

    fn visit_none<E: de::Error>(self) -> Result<YamlValue, E> {
        Ok(YamlValue::Null)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<YamlValue, D::Error> {
        Deserialize::deserialize(deserializer)
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<YamlValue, E> {
        Ok(YamlValue::Bool(v))
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<YamlValue, E> {
        Ok(YamlValue::Int(v))
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<YamlValue, E> {
        if v <= i64::MAX as u64 {
            Ok(YamlValue::Int(v as i64))
        } else {
            Ok(YamlValue::Float(v as f64))
        }
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<YamlValue, E> {
        Ok(YamlValue::Float(v))
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<YamlValue, E> {
        Ok(YamlValue::String(v.to_owned()))
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<YamlValue, E> {
        Ok(YamlValue::String(v))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<YamlValue, A::Error> {
        let mut values = Vec::with_capacity(seq.size_hint().unwrap_or(0));
        while let Some(elem) = seq.next_element()? {
            values.push(elem);
        }
        Ok(YamlValue::Sequence(values))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<YamlValue, A::Error> {
        let mut values = IndexMap::with_capacity(map.size_hint().unwrap_or(0));
        while let Some((key, value)) = map.next_entry::<String, YamlValue>()? {
            values.insert(key, value);
        }
        Ok(YamlValue::Mapping(values))
    }
}

// ============================================================================
// Convenience methods
// ============================================================================

impl YamlValue {
    /// Returns the string value, if this is a `String`.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            YamlValue::String(s) => Some(s),
            _ => None,
        }
    }

    /// Returns the boolean value, if this is a `Bool`.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            YamlValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Returns the value as `i64`, if this is an `Int` or a lossless `Float`.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            YamlValue::Int(i) => Some(*i),
            YamlValue::Float(f) => {
                let i = *f as i64;
                if (i as f64) == *f { Some(i) } else { None }
            }
            _ => None,
        }
    }

    /// Returns the value as `u64`, if this is a non-negative `Int`.
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            YamlValue::Int(i) if *i >= 0 => Some(*i as u64),
            _ => None,
        }
    }

    /// Returns the value as `f64`, if this is a `Float` or `Int`.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            YamlValue::Float(f) => Some(*f),
            YamlValue::Int(i) => Some(*i as f64),
            _ => None,
        }
    }

    /// Returns the sequence, if this is a `Sequence`.
    pub fn as_sequence(&self) -> Option<&Vec<YamlValue>> {
        match self {
            YamlValue::Sequence(v) => Some(v),
            _ => None,
        }
    }

    /// Returns a mutable reference to the sequence, if this is a `Sequence`.
    pub fn as_sequence_mut(&mut self) -> Option<&mut Vec<YamlValue>> {
        match self {
            YamlValue::Sequence(v) => Some(v),
            _ => None,
        }
    }

    /// Returns the mapping, if this is a `Mapping`.
    pub fn as_mapping(&self) -> Option<&IndexMap<String, YamlValue>> {
        match self {
            YamlValue::Mapping(m) => Some(m),
            _ => None,
        }
    }

    /// Returns a mutable reference to the mapping, if this is a `Mapping`.
    pub fn as_mapping_mut(&mut self) -> Option<&mut IndexMap<String, YamlValue>> {
        match self {
            YamlValue::Mapping(m) => Some(m),
            _ => None,
        }
    }

    /// Returns `true` if this value is `Null`.
    pub fn is_null(&self) -> bool {
        matches!(self, YamlValue::Null)
    }

    /// Returns `true` if this value is a `String`.
    pub fn is_string(&self) -> bool {
        matches!(self, YamlValue::String(_))
    }

    /// Returns `true` if this value is a `Sequence`.
    pub fn is_sequence(&self) -> bool {
        matches!(self, YamlValue::Sequence(_))
    }

    /// Returns `true` if this value is a `Mapping`.
    pub fn is_mapping(&self) -> bool {
        matches!(self, YamlValue::Mapping(_))
    }

    /// Returns `true` if this value is a number (`Int` or `Float`).
    pub fn is_number(&self) -> bool {
        matches!(self, YamlValue::Int(_) | YamlValue::Float(_))
    }

    /// Access a mapping value by key.
    pub fn get(&self, key: &str) -> Option<&YamlValue> {
        self.as_mapping().and_then(|m| m.get(key))
    }
}

impl std::fmt::Display for YamlValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            YamlValue::Null => write!(f, "null"),
            YamlValue::Bool(b) => write!(f, "{b}"),
            YamlValue::Int(i) => write!(f, "{i}"),
            YamlValue::Float(v) => write!(f, "{v}"),
            YamlValue::String(s) => write!(f, "{s}"),
            YamlValue::Sequence(_) | YamlValue::Mapping(_) => match serde_yaml::to_string(self) {
                Ok(s) => write!(f, "{}", s.trim()),
                Err(_) => write!(f, "<complex value>"),
            },
        }
    }
}

// ============================================================================
// Conversions
// ============================================================================

impl From<serde_json::Value> for YamlValue {
    fn from(json: serde_json::Value) -> Self {
        match json {
            serde_json::Value::Null => YamlValue::Null,
            serde_json::Value::Bool(b) => YamlValue::Bool(b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    YamlValue::Int(i)
                } else if let Some(f) = n.as_f64() {
                    YamlValue::Float(f)
                } else {
                    YamlValue::Null
                }
            }
            serde_json::Value::String(s) => YamlValue::String(s),
            serde_json::Value::Array(arr) => {
                YamlValue::Sequence(arr.into_iter().map(YamlValue::from).collect())
            }
            serde_json::Value::Object(map) => {
                let m: IndexMap<String, YamlValue> = map
                    .into_iter()
                    .map(|(k, v)| (k, YamlValue::from(v)))
                    .collect();
                YamlValue::Mapping(m)
            }
        }
    }
}

impl From<YamlValue> for serde_json::Value {
    fn from(yaml: YamlValue) -> Self {
        match yaml {
            YamlValue::Null => serde_json::Value::Null,
            YamlValue::Bool(b) => serde_json::Value::Bool(b),
            YamlValue::Int(i) => serde_json::Value::Number(i.into()),
            YamlValue::Float(f) => serde_json::Number::from_f64(f)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
            YamlValue::String(s) => serde_json::Value::String(s),
            YamlValue::Sequence(arr) => {
                serde_json::Value::Array(arr.into_iter().map(serde_json::Value::from).collect())
            }
            YamlValue::Mapping(map) => {
                let obj: serde_json::Map<String, serde_json::Value> = map
                    .into_iter()
                    .map(|(k, v)| (k, serde_json::Value::from(v)))
                    .collect();
                serde_json::Value::Object(obj)
            }
        }
    }
}

impl From<&str> for YamlValue {
    fn from(s: &str) -> Self {
        YamlValue::String(s.to_string())
    }
}

impl From<String> for YamlValue {
    fn from(s: String) -> Self {
        YamlValue::String(s)
    }
}

impl From<bool> for YamlValue {
    fn from(b: bool) -> Self {
        YamlValue::Bool(b)
    }
}

impl From<i64> for YamlValue {
    fn from(i: i64) -> Self {
        YamlValue::Int(i)
    }
}

impl From<f64> for YamlValue {
    fn from(f: f64) -> Self {
        YamlValue::Float(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_yaml() {
        let yaml = "title: Hello\ncount: 42\ntags:\n- a\n- b\n";
        let value: YamlValue = serde_yaml::from_str(yaml).unwrap();

        assert!(value.is_mapping());
        let map = value.as_mapping().unwrap();
        assert_eq!(map.get("title").unwrap().as_str(), Some("Hello"));
        assert_eq!(map.get("count").unwrap().as_i64(), Some(42));
        assert!(map.get("tags").unwrap().is_sequence());
    }

    #[test]
    fn json_round_trip() {
        let yaml_val = YamlValue::Mapping(IndexMap::from([
            ("key".to_string(), YamlValue::String("value".to_string())),
            ("num".to_string(), YamlValue::Int(42)),
        ]));

        let json: serde_json::Value = yaml_val.clone().into();
        let back: YamlValue = json.into();
        assert_eq!(yaml_val, back);
    }
}
