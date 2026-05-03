//! YAML frontmatter parsing and manipulation.
//!
//! Functions in this module work on markdown content delimited by a pair of
//! `---` fences and a YAML body in between. For format-only parsing (no
//! delimiter handling), see [`crate::yaml`].

use indexmap::IndexMap;
use thiserror::Error;

use crate::yaml::{self, Value};

/// Errors that can occur while parsing or serializing YAML frontmatter.
#[derive(Debug, Error)]
pub enum FrontmatterError {
    /// The input did not contain valid frontmatter delimiters (`---` ... `---`).
    #[error("file has no frontmatter delimiters")]
    NoFrontmatter,

    /// The frontmatter YAML failed to parse or serialize.
    #[error("YAML parsing error: {0}")]
    Yaml(#[from] yaml::Error),
}

/// Convenience `Result` alias parameterized by [`FrontmatterError`].
pub type Result<T> = std::result::Result<T, FrontmatterError>;

/// Result of parsing a markdown file with frontmatter.
#[derive(Debug, Clone)]
pub struct ParsedFile {
    /// The parsed frontmatter as an ordered map.
    pub frontmatter: IndexMap<String, Value>,
    /// The body content after the frontmatter.
    pub body: String,
}

/// Parse frontmatter and body from markdown content.
///
/// Returns `Ok(ParsedFile)` with the frontmatter and body.
/// Returns `Err(NoFrontmatter)` if the content doesn't have valid frontmatter delimiters.
pub fn parse(content: &str) -> Result<ParsedFile> {
    if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
        return Err(FrontmatterError::NoFrontmatter);
    }

    let rest = &content[4..]; // Skip first "---\n"
    let end_idx = rest
        .find("\n---\n")
        .or_else(|| rest.find("\n---\r\n"))
        .ok_or(FrontmatterError::NoFrontmatter)?;

    let frontmatter_str = &rest[..end_idx];
    let body = &rest[end_idx + 5..]; // Skip "\n---\n"

    let frontmatter: IndexMap<String, Value> = yaml::from_str(frontmatter_str)?;

    Ok(ParsedFile {
        frontmatter,
        body: body.to_string(),
    })
}

/// Parse frontmatter and body, returning empty frontmatter if none exists.
///
/// Unlike `parse()`, this function never returns an error for missing frontmatter.
/// Use this for operations that should work on files without frontmatter.
pub fn parse_or_empty(content: &str) -> Result<ParsedFile> {
    if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
        return Ok(ParsedFile {
            frontmatter: IndexMap::new(),
            body: content.to_string(),
        });
    }

    let rest = &content[4..]; // Skip first "---\n"
    let end_idx = rest.find("\n---\n").or_else(|| rest.find("\n---\r\n"));

    match end_idx {
        Some(idx) => {
            let frontmatter_str = &rest[..idx];
            let body = &rest[idx + 5..]; // Skip "\n---\n"

            let frontmatter: IndexMap<String, Value> = yaml::from_str(frontmatter_str)?;

            Ok(ParsedFile {
                frontmatter,
                body: body.to_string(),
            })
        }
        None => {
            // Malformed frontmatter (no closing delimiter) - treat as no frontmatter
            Ok(ParsedFile {
                frontmatter: IndexMap::new(),
                body: content.to_string(),
            })
        }
    }
}

/// Serialize frontmatter and body back to markdown content.
pub fn serialize(frontmatter: &IndexMap<String, Value>, body: &str) -> Result<String> {
    let yaml_str = yaml::to_string(frontmatter)?;
    Ok(format!("---\n{}---\n{}", yaml_str, body))
}

/// Extract the raw YAML string from between frontmatter delimiters.
///
/// Returns the YAML text without the `---` delimiters, or `None` if no
/// valid frontmatter delimiters are found.
pub fn extract_yaml(content: &str) -> Option<&str> {
    split(content).map(|(yaml, _)| yaml)
}

/// Split a markdown string into `(frontmatter_yaml, body)` without parsing
/// the YAML. Returns `None` if `content` does not start with a `---` opening
/// delimiter or has no closing `---` delimiter.
///
/// Slicing is byte-identical to [`parse`] / [`parse_or_empty`]: this is the
/// shared primitive that all delimiter-based extraction in this module uses.
///
/// Use this when you want to defer YAML deserialization to the caller — for
/// example, to deserialize the frontmatter into a typed Serde struct rather
/// than the dynamic [`Value`].
pub fn split(content: &str) -> Option<(&str, &str)> {
    if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
        return None;
    }
    let rest = &content[4..]; // matches parse()/parse_or_empty()
    let end = rest.find("\n---\n").or_else(|| rest.find("\n---\r\n"))?;
    let yaml = &rest[..end];
    let body = &rest[end + 5..];
    Some((yaml, body))
}

/// Parse a typed struct from YAML frontmatter in a markdown file.
///
/// Extracts the YAML between `---` delimiters and deserializes it into `T`.
/// If no frontmatter delimiters are found, attempts to parse the entire content as YAML.
pub fn parse_typed<T: serde::de::DeserializeOwned>(
    content: &str,
) -> std::result::Result<T, yaml::Error> {
    let s = extract_yaml(content).unwrap_or(content);
    yaml::from_str(s)
}

/// Serialize a typed struct as YAML frontmatter in a markdown file.
pub fn serialize_typed<T: serde::Serialize>(value: &T) -> std::result::Result<String, yaml::Error> {
    let s = yaml::to_string(value)?;
    Ok(format!("---\n{}---\n", s))
}

/// Serialize any Serde-serializable value as YAML frontmatter, with the given body.
///
/// Like [`serialize`], but accepts any `T: Serialize` rather than requiring the
/// dynamic [`Mapping`]. Useful for round-tripping typed frontmatter structs.
pub fn serialize_with_body<T: serde::Serialize>(
    value: &T,
    body: &str,
) -> std::result::Result<String, yaml::Error> {
    let s = yaml::to_string(value)?;
    Ok(format!("---\n{}---\n{}", s, body))
}

/// Extract only the body from markdown content, stripping frontmatter.
///
/// If no frontmatter exists, returns the content unchanged.
pub fn extract_body(content: &str) -> &str {
    if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
        return content;
    }

    let rest = &content[4..];
    if let Some(end_idx) = rest.find("\n---\n").or_else(|| rest.find("\n---\r\n")) {
        let body_start = end_idx + 5;
        if body_start < rest.len() {
            &rest[body_start..]
        } else {
            ""
        }
    } else {
        content
    }
}

/// Get a property from frontmatter.
pub fn get_property<'a>(frontmatter: &'a IndexMap<String, Value>, key: &str) -> Option<&'a Value> {
    frontmatter.get(key)
}

/// Set a property in frontmatter (in place).
pub fn set_property(frontmatter: &mut IndexMap<String, Value>, key: &str, value: Value) {
    frontmatter.insert(key.to_string(), value);
}

/// Remove a property from frontmatter (in place).
pub fn remove_property(frontmatter: &mut IndexMap<String, Value>, key: &str) -> Option<Value> {
    frontmatter.shift_remove(key)
}

/// Get a string property value.
pub fn get_string<'a>(frontmatter: &'a IndexMap<String, Value>, key: &str) -> Option<&'a str> {
    frontmatter.get(key).and_then(|v| v.as_str())
}

/// Get an array property as a Vec of strings.
pub fn get_string_array(frontmatter: &IndexMap<String, Value>, key: &str) -> Vec<String> {
    match frontmatter.get(key) {
        Some(Value::Sequence(seq)) => seq
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        _ => Vec::new(),
    }
}

/// Replace only the body portion of a markdown string, preserving the raw
/// frontmatter block byte-for-byte. This avoids a YAML round-trip.
///
/// If `content` has no frontmatter (or has a malformed opening/closing
/// delimiter), returns `new_body` as-is.
pub fn replace_body(content: &str, new_body: &str) -> String {
    let open_len = if content.starts_with("---\n") {
        4
    } else if content.starts_with("---\r\n") {
        5
    } else {
        return new_body.to_string();
    };

    let rest = &content[open_len..];

    let header_end = if let Some(idx) = rest.find("\n---\n") {
        open_len + idx + 5 // through "\n---\n"
    } else if let Some(idx) = rest.find("\n---\r\n") {
        open_len + idx + 6 // through "\n---\r\n"
    } else {
        return new_body.to_string();
    };

    format!("{}\n{}", &content[..header_end], new_body)
}

/// Sort frontmatter keys alphabetically.
pub fn sort_alphabetically(frontmatter: IndexMap<String, Value>) -> IndexMap<String, Value> {
    let mut pairs: Vec<_> = frontmatter.into_iter().collect();
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    pairs.into_iter().collect()
}

/// Sort frontmatter keys according to a pattern.
///
/// Pattern is comma-separated keys, with "*" meaning "rest alphabetically".
/// Example: "title,description,*" puts title first, description second, rest alphabetically
pub fn sort_by_pattern(
    frontmatter: IndexMap<String, Value>,
    pattern: &str,
) -> IndexMap<String, Value> {
    let priority_keys: Vec<&str> = pattern.split(',').map(|s| s.trim()).collect();

    let mut result = IndexMap::new();
    let mut remaining = frontmatter;

    for key in &priority_keys {
        if *key == "*" {
            let mut rest: Vec<_> = remaining.drain(..).collect();
            rest.sort_by(|a, b| a.0.cmp(&b.0));
            for (k, v) in rest {
                result.insert(k, v);
            }
            break;
        } else if let Some(value) = remaining.shift_remove(*key) {
            result.insert(key.to_string(), value);
        }
    }

    if !remaining.is_empty() {
        let mut rest: Vec<_> = remaining.drain(..).collect();
        rest.sort_by(|a, b| a.0.cmp(&b.0));
        for (k, v) in rest {
            result.insert(k, v);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_frontmatter() {
        let content = "---\ntitle: Test\n---\n\nBody content";
        let parsed = parse(content).unwrap();
        assert_eq!(
            parsed.frontmatter.get("title").unwrap().as_str().unwrap(),
            "Test"
        );
        assert_eq!(parsed.body.trim(), "Body content");
    }

    #[test]
    fn test_parse_no_frontmatter() {
        let content = "Just body content";
        let result = parse(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_or_empty_no_frontmatter() {
        let content = "Just body content";
        let parsed = parse_or_empty(content).unwrap();
        assert!(parsed.frontmatter.is_empty());
        assert_eq!(parsed.body, content);
    }

    #[test]
    fn test_serialize() {
        let mut fm = IndexMap::new();
        fm.insert("title".to_string(), Value::String("Test".to_string()));
        let result = serialize(&fm, "\nBody").unwrap();
        assert!(result.starts_with("---\n"));
        assert!(result.contains("title: Test"));
        assert!(result.contains("---\n\nBody"));
    }

    #[test]
    fn test_extract_body() {
        let content = "---\ntitle: Test\n---\n\nBody content";
        assert_eq!(extract_body(content).trim(), "Body content");
    }

    #[test]
    fn test_extract_body_no_frontmatter() {
        let content = "Just body content";
        assert_eq!(extract_body(content), content);
    }

    #[test]
    fn test_sort_alphabetically() {
        let mut fm = IndexMap::new();
        fm.insert("zebra".to_string(), Value::Null);
        fm.insert("apple".to_string(), Value::Null);
        fm.insert("banana".to_string(), Value::Null);

        let sorted = sort_alphabetically(fm);
        let keys: Vec<_> = sorted.keys().collect();
        assert_eq!(keys, vec!["apple", "banana", "zebra"]);
    }

    #[test]
    fn test_replace_body_with_frontmatter() {
        let content = "---\ntitle: Test\n---\n\nOld body";
        let result = replace_body(content, "New body");
        assert_eq!(result, "---\ntitle: Test\n---\n\nNew body");
    }

    #[test]
    fn test_replace_body_no_frontmatter() {
        let result = replace_body("Just body", "New body");
        assert_eq!(result, "New body");
    }

    #[test]
    fn test_replace_body_empty_new_body() {
        let content = "---\ntitle: Test\n---\n\nOld body";
        let result = replace_body(content, "");
        assert_eq!(result, "---\ntitle: Test\n---\n\n");
    }

    #[test]
    fn test_replace_body_crlf() {
        let content = "---\r\ntitle: Test\r\n---\r\n\r\nOld body";
        let result = replace_body(content, "New body");
        assert_eq!(result, "---\r\ntitle: Test\r\n---\r\n\nNew body");
    }

    #[test]
    fn test_replace_body_malformed() {
        let content = "---\nunclosed frontmatter";
        let result = replace_body(content, "New body");
        assert_eq!(result, "New body");
    }

    #[test]
    fn test_replace_body_preserves_formatting() {
        let content = "---\ntitle: \"Quoted Title\"\ntags:\n  - rust\n  - swift\n---\n\nOld body";
        let result = replace_body(content, "New body");
        assert!(
            result.starts_with("---\ntitle: \"Quoted Title\"\ntags:\n  - rust\n  - swift\n---\n")
        );
        assert!(result.ends_with("\nNew body"));
    }

    #[test]
    fn test_sort_by_pattern() {
        let mut fm = IndexMap::new();
        fm.insert("zebra".to_string(), Value::Null);
        fm.insert("title".to_string(), Value::Null);
        fm.insert("apple".to_string(), Value::Null);

        let sorted = sort_by_pattern(fm, "title,*");
        let keys: Vec<_> = sorted.keys().collect();
        assert_eq!(keys, vec!["title", "apple", "zebra"]);
    }
}
