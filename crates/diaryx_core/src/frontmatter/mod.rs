//! YAML frontmatter parsing and manipulation.
//!
//! Functions in this module work on markdown content delimited by a pair of
//! `---` fences and a YAML body in between. For format-only parsing (no
//! delimiter handling), see [`crate::yaml`].

use fig::{Embed, Segment};
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

    let frontmatter = yaml::parse_mapping(frontmatter_str)?;

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

            let frontmatter = yaml::parse_mapping(frontmatter_str)?;

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
    let yaml_str = yaml::serialize_mapping(frontmatter)?;
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
/// frontmatter block (and its closing fence) byte-for-byte. This avoids a YAML
/// round-trip, so comments, key order, and formatting in the frontmatter are
/// untouched.
///
/// `new_body` replaces everything after the closing `---` fence's newline —
/// exactly the slice [`split`]/[`extract_body`] return as the body — so a
/// `replace_body(c, extract_body(c))` round-trip is the identity.
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

    format!("{}{}", &content[..header_end], new_body)
}

// ============================================================================
// Comment-preserving in-place edits (fig backend)
//
// Unlike [`serialize`], which reserializes the whole frontmatter (discarding
// comments and original formatting), these edit only the targeted node's bytes
// via fig's frontmatter editor — comments, key order, quoting, and blank lines
// everywhere else stay byte-identical. Each takes the full markdown text and
// returns the new text. They operate on top-level frontmatter keys (the shape
// Diaryx's property commands use).
// ============================================================================

/// Set (insert-or-replace) a top-level frontmatter property, preserving the
/// rest of the document (comments, key order, formatting). Creates a
/// frontmatter block when the document has none.
///
/// fig's editor reframes inline↔block when replacing a mapping value (a scalar
/// stays inline; a sequence/mapping descends onto its own lines), so both scalar
/// and collection values edit in place and keep surrounding comments. The
/// wholesale reserialize stays as a fallback for shapes fig can't splice
/// (creating frontmatter where there is none, or a value fig's editor declines)
/// — always valid YAML, though not comment-preserving for that one write.
pub fn set_property_in_text(content: &str, key: &str, value: &Value) -> Result<String> {
    // Fast path: when the property is a list of scalars (the shape of Diaryx's
    // `contents` / `part_of` / `links` / … link lists), edit the sequence with
    // fig's per-item primitives instead of replacing the whole node, so the
    // comments attached to individual surviving items are preserved. Declines
    // (returns `None`) for any shape it can't safely diff, falling through to
    // the whole-value `replace`/`insert` path below.
    if let Value::Sequence(new_seq) = value
        && let Some(out) = try_seq_item_edit(content, key, new_seq)
    {
        return Ok(out);
    }

    match Embed::frontmatter(content.as_bytes()) {
        Ok(mut fm) => {
            // Replace in place if the key exists, otherwise insert it. Both edit
            // only the targeted node's bytes, so comments and formatting
            // elsewhere are preserved. A genuine fig error (not "key absent")
            // is surfaced rather than papered over with a comment-dropping
            // reserialize — the old reserialize fallback would have failed on
            // the same input anyway.
            match fm.replace_value(&[Segment::Key(key)], &value.into()) {
                Ok(()) => {}
                Err(fig::Error::NotFound) => fm.insert_value(&[], key, &value.into())?,
                Err(e) => return Err(e.into()),
            }
            Ok(fm.render()?.to_string())
        }
        // The document has no frontmatter block: create one. This is the only
        // reserialize, and it is lossless — a document with no frontmatter has
        // no comments or key ordering to preserve.
        Err(fig::Error::NotFound) => create_frontmatter_block(content, key, value),
        Err(e) => Err(e.into()),
    }
}

/// True for YAML scalar values (no nested structure). The per-item sequence
/// edit only matches items by value identity, which is only well-defined for
/// scalars.
fn is_scalar(v: &Value) -> bool {
    matches!(
        v,
        Value::Null | Value::Bool(_) | Value::Int(_) | Value::Float(_) | Value::String(_)
    )
}

/// A type-tagged identity string for a scalar, so distinct YAML types with the
/// same text (`1` vs `"1"` vs `true`) never compare equal.
fn scalar_ident(v: &Value) -> String {
    match v {
        Value::Null => "n:".to_string(),
        Value::Bool(b) => format!("b:{b}"),
        Value::Int(i) => format!("i:{i}"),
        Value::Float(f) => format!("f:{f}"),
        Value::String(s) => format!("s:{s}"),
        // Gated to scalars by the caller.
        _ => String::new(),
    }
}

/// Occurrence-tagged identity keys: the k-th item with a given identity gets
/// tag `(ident, k)`, so duplicate values are matched 1:1 between the old and
/// new sequences rather than ambiguously by set membership.
fn occurrence_keys(items: &[Value]) -> Vec<(String, usize)> {
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    items
        .iter()
        .map(|v| {
            let id = scalar_ident(v);
            let n = seen.entry(id.clone()).or_insert(0);
            let k = *n;
            *n += 1;
            (id, k)
        })
        .collect()
}

/// Apply a new scalar-sequence value to `key` using fig's per-item primitives
/// (`remove_item` / `append` / `reorder_items`) so surviving items keep their
/// comments. Returns `Some(rendered)` on success, or `None` to decline — the
/// caller then falls back to the whole-value replace path.
///
/// Declines when the edit isn't a safe item-level diff: the key is absent, the
/// existing value isn't a non-empty scalar sequence, the new value isn't a
/// non-empty scalar sequence, or every existing item would be removed (which
/// would transiently empty the block list, a shape fig's item ops can't hold).
///
/// Known ambiguity: when duplicate values are dropped (old has N copies, new
/// has M < N), the first M occurrences are kept as survivors and the trailing
/// N−M removed — so a comment owned by a removed duplicate may end up beside a
/// surviving equal-valued item. This is benign for link lists.
fn try_seq_item_edit(content: &str, key: &str, new_seq: &[Value]) -> Option<String> {
    use std::collections::{HashMap, HashSet};

    // New value must be a non-empty list of scalars.
    if new_seq.is_empty() || !new_seq.iter().all(is_scalar) {
        return None;
    }

    let parsed = parse_or_empty(content).ok()?;
    // Key must already exist as a non-empty scalar sequence.
    let old_seq = match parsed.frontmatter.get(key)? {
        Value::Sequence(s) if !s.is_empty() && s.iter().all(is_scalar) => s,
        _ => return None,
    };

    let old_keys = occurrence_keys(old_seq);
    let new_keys = occurrence_keys(new_seq);
    let old_set: HashSet<&(String, usize)> = old_keys.iter().collect();
    let new_set: HashSet<&(String, usize)> = new_keys.iter().collect();

    // Old positions whose value no longer appears (with matching multiplicity).
    let removed: HashSet<usize> = (0..old_keys.len())
        .filter(|i| !new_set.contains(&old_keys[*i]))
        .collect();
    // Removing every item would leave fig with an empty block list mid-edit.
    if removed.len() == old_seq.len() {
        return None;
    }
    // New positions whose value wasn't present before, in new order.
    let additions: Vec<usize> = (0..new_keys.len())
        .filter(|j| !old_set.contains(&new_keys[*j]))
        .collect();

    // The sequence after removals (survivors in old order) then appends
    // (additions in new order) — this is the order fig will be in before the
    // final reorder.
    let mut current_keys: Vec<&(String, usize)> = Vec::with_capacity(new_keys.len());
    for (i, k) in old_keys.iter().enumerate() {
        if !removed.contains(&i) {
            current_keys.push(k);
        }
    }
    for &j in &additions {
        current_keys.push(&new_keys[j]);
    }

    // Target permutation: for each new position, where that item currently sits.
    let mut pos: HashMap<&(String, usize), usize> = HashMap::with_capacity(current_keys.len());
    for (idx, k) in current_keys.iter().enumerate() {
        pos.insert(*k, idx);
    }
    let mut order: Vec<usize> = Vec::with_capacity(new_keys.len());
    for k in &new_keys {
        order.push(*pos.get(k)?);
    }
    // `order` must be a full permutation of current positions, or fig's
    // out-of-range-ignoring reorder would silently produce a wrong document.
    if order.len() != current_keys.len() {
        return None;
    }

    // Pure no-op (same items, same order): return the input untouched so a
    // redundant set is byte-identical and never churns formatting.
    let is_identity = removed.is_empty()
        && additions.is_empty()
        && order.iter().enumerate().all(|(i, &o)| i == o);
    if is_identity {
        return Some(content.to_string());
    }

    // Apply on a fresh editor; any fig error → decline and fall back.
    let mut fm = Embed::frontmatter(content.as_bytes()).ok()?;
    // Remove high indices first so lower indices stay valid.
    let mut removals_desc: Vec<usize> = removed.into_iter().collect();
    removals_desc.sort_unstable_by(|a, b| b.cmp(a));
    for idx in removals_desc {
        fm.remove_item(&[Segment::Key(key)], idx).ok()?;
    }
    for &j in &additions {
        fm.append_value(&[Segment::Key(key)], &(&new_seq[j]).into())
            .ok()?;
    }
    if order.iter().enumerate().any(|(i, &o)| i != o) {
        fm.reorder_items(&[Segment::Key(key)], &order).ok()?;
    }
    Some(fm.render().ok()?.to_string())
}

/// Create a frontmatter block for a document that has none, with `key: value`
/// as its sole property, preserving the original body. Used only when
/// [`Embed::frontmatter`] reports no frontmatter — so there is nothing to
/// preserve and the serialize is lossless. (For a document that already has
/// frontmatter, [`set_property_in_text`] edits it in place via fig instead.)
fn create_frontmatter_block(content: &str, key: &str, value: &Value) -> Result<String> {
    let parsed = parse_or_empty(content)?;
    let mut map = parsed.frontmatter;
    map.insert(key.to_string(), value.clone());
    serialize(&map, &parsed.body)
}

/// Remove a top-level frontmatter property. Returns the text unchanged when the
/// document has no frontmatter or the key is absent.
pub fn remove_property_in_text(content: &str, key: &str) -> Result<String> {
    match Embed::frontmatter(content.as_bytes()) {
        Ok(mut fm) => match fm.delete(&[Segment::Key(key)]) {
            Ok(()) => Ok(fm.render()?.to_string()),
            Err(fig::Error::NotFound) => Ok(content.to_string()),
            Err(e) => Err(e.into()),
        },
        Err(fig::Error::NotFound) => Ok(content.to_string()),
        Err(e) => Err(e.into()),
    }
}

/// Rename a top-level frontmatter key in place (value and position preserved).
/// Returns `None` when the document has no frontmatter or the key is absent.
pub fn rename_property_in_text(
    content: &str,
    old_key: &str,
    new_key: &str,
) -> Result<Option<String>> {
    match Embed::frontmatter(content.as_bytes()) {
        Ok(mut fm) => match fm.replace_key(&[Segment::Key(old_key)], new_key) {
            Ok(()) => Ok(Some(fm.render()?.to_string())),
            Err(fig::Error::NotFound) => Ok(None),
            Err(e) => Err(e.into()),
        },
        Err(fig::Error::NotFound) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Reorder top-level frontmatter keys to match `keys` (listed keys first, in
/// that order; keys not listed keep their original relative order and follow),
/// preserving comments and formatting via fig's in-place editor. Keys in `keys`
/// that the frontmatter does not contain are ignored. Returns the text unchanged
/// when the document has no frontmatter.
pub fn reorder_keys_in_text(content: &str, keys: &[String]) -> Result<String> {
    match Embed::frontmatter(content.as_bytes()) {
        Ok(mut fm) => {
            fm.reorder_keys(&[] as &[Segment], keys)?;
            Ok(fm.render()?.to_string())
        }
        Err(fig::Error::NotFound) => Ok(content.to_string()),
        Err(e) => Err(e.into()),
    }
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
    fn test_replace_body_preserves_frontmatter_comments() {
        // A comment in the frontmatter must survive a body-only write.
        let content = "---\ntitle: Test\n# keep this comment\ntags:\n- x\n---\noriginal body\n";
        let updated = replace_body(content, "new body\n");
        assert_eq!(
            updated,
            "---\ntitle: Test\n# keep this comment\ntags:\n- x\n---\nnew body\n",
        );
    }

    #[test]
    fn test_replace_body_round_trips_with_extract_body() {
        // replace_body(c, extract_body(c)) is the identity (no spurious blank line).
        let content = "---\ntitle: Test\n---\n\nBody content\n";
        assert_eq!(replace_body(content, extract_body(content)), content);
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
        // The body is everything after the closing fence's newline and owns its
        // own leading blank line, so the new body is spliced in verbatim (this
        // matches the slice `parse`/`extract_body` return as the body).
        let content = "---\ntitle: Test\n---\n\nOld body";
        let result = replace_body(content, "\nNew body");
        assert_eq!(result, "---\ntitle: Test\n---\n\nNew body");
    }

    #[test]
    fn test_replace_body_no_frontmatter() {
        let result = replace_body("Just body", "New body");
        assert_eq!(result, "New body");
    }

    #[test]
    fn test_replace_body_empty_new_body() {
        // Clearing the body leaves just the frontmatter block + closing fence.
        let content = "---\ntitle: Test\n---\n\nOld body";
        let result = replace_body(content, "");
        assert_eq!(result, "---\ntitle: Test\n---\n");
    }

    #[test]
    fn test_replace_body_crlf() {
        let content = "---\r\ntitle: Test\r\n---\r\n\r\nOld body";
        let result = replace_body(content, "\r\nNew body");
        assert_eq!(result, "---\r\ntitle: Test\r\n---\r\n\r\nNew body");
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

    // ---- comment-preserving in-place edits ----

    const NOTE: &str =
        "---\ntitle: Hello\n# keep this comment\ntags:\n- a\n- b\n---\n# Body\n\nprose\n";

    #[test]
    fn set_property_replaces_preserving_comment_and_body() {
        let out = set_property_in_text(NOTE, "title", &Value::String("Hi there".into())).unwrap();
        assert!(out.contains("title: Hi there"));
        assert!(out.contains("# keep this comment"));
        assert!(out.contains("prose"));
        // Re-reading reflects the change.
        let parsed = parse(&out).unwrap();
        assert_eq!(
            parsed.frontmatter.get("title").unwrap().as_str(),
            Some("Hi there")
        );
    }

    #[test]
    fn set_property_inserts_new_key() {
        let out = set_property_in_text(NOTE, "author", &Value::String("me".into())).unwrap();
        let parsed = parse(&out).unwrap();
        assert_eq!(
            parsed.frontmatter.get("author").unwrap().as_str(),
            Some("me")
        );
        assert!(out.contains("# keep this comment"));
    }

    #[test]
    fn set_property_creates_frontmatter_when_missing() {
        let out = set_property_in_text("just body\n", "title", &Value::String("T".into())).unwrap();
        let parsed = parse(&out).unwrap();
        assert_eq!(parsed.frontmatter.get("title").unwrap().as_str(), Some("T"));
        assert!(parsed.body.contains("just body"));
    }

    #[test]
    fn set_collection_property_preserves_comments() {
        // Replacing an array value (a collection) keeps comments elsewhere —
        // relies on fig's editor reframing the value in place rather than the
        // whole-document reserialize fallback.
        let tags = Value::Sequence(vec![Value::String("x".into()), Value::String("y".into())]);
        let out = set_property_in_text(NOTE, "tags", &tags).unwrap();
        assert!(out.contains("# keep this comment"), "comment lost: {out}");
        let parsed = parse(&out).unwrap();
        let got = parsed
            .frontmatter
            .get("tags")
            .unwrap()
            .as_sequence()
            .unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].as_str(), Some("x"));
        // A freshly-populated empty array reframes inline `[]` -> a block list.
        let seeded = "---\ntitle: T\n# c\nitems: []\n---\nbody\n";
        let out2 = set_property_in_text(seeded, "items", &tags).unwrap();
        assert!(out2.contains("# c"));
        assert_eq!(
            parse(&out2)
                .unwrap()
                .frontmatter
                .get("items")
                .unwrap()
                .as_sequence()
                .unwrap()
                .len(),
            2
        );
    }

    // ---- Per-item sequence edits (comment-preserving) ----

    fn seq(items: &[&str]) -> Value {
        Value::Sequence(items.iter().map(|s| Value::String((*s).into())).collect())
    }

    fn list_of(out: &str, key: &str) -> Vec<String> {
        parse(out)
            .unwrap()
            .frontmatter
            .get(key)
            .unwrap()
            .as_sequence()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    }

    // A note whose `contents` items carry their own inline/leading comments.
    const CONTENTS: &str =
        "---\ntitle: T\ncontents:\n- a # note a\n# leading c\n- c\n- b\n---\nbody\n";

    #[test]
    fn seq_reorder_preserves_item_comments() {
        let out = set_property_in_text(CONTENTS, "contents", &seq(&["c", "a", "b"])).unwrap();
        assert_eq!(list_of(&out, "contents"), vec!["c", "a", "b"]);
        // Item comments ride with their items.
        assert!(out.contains("# note a"), "lost inline comment: {out}");
        assert!(out.contains("# leading c"), "lost leading comment: {out}");
    }

    #[test]
    fn seq_remove_middle_keeps_other_comments() {
        // Remove `c`; `a`'s inline comment must survive.
        let out = set_property_in_text(CONTENTS, "contents", &seq(&["a", "b"])).unwrap();
        assert_eq!(list_of(&out, "contents"), vec!["a", "b"]);
        assert!(out.contains("# note a"));
        // The removed item carried its own leading comment, which goes with it.
        assert!(!out.contains("- c"));
    }

    #[test]
    fn seq_append_keeps_existing_comments() {
        let out = set_property_in_text(CONTENTS, "contents", &seq(&["a", "c", "b", "d"])).unwrap();
        assert_eq!(list_of(&out, "contents"), vec!["a", "c", "b", "d"]);
        assert!(out.contains("# note a"));
        assert!(out.contains("# leading c"));
    }

    #[test]
    fn seq_add_remove_reorder_combined() {
        // old [a, c, b] -> new [b, d, a]: remove c, add d, reorder.
        let out = set_property_in_text(CONTENTS, "contents", &seq(&["b", "d", "a"])).unwrap();
        assert_eq!(list_of(&out, "contents"), vec!["b", "d", "a"]);
        assert!(
            out.contains("# note a"),
            "comment on surviving `a` lost: {out}"
        );
    }

    #[test]
    fn seq_duplicate_survivor_and_removed_duplicate() {
        let src = "---\nx:\n- a # first\n- a # second\n- a # third\n---\nbody\n";
        let out = set_property_in_text(src, "x", &seq(&["a", "a"])).unwrap();
        assert_eq!(list_of(&out, "x"), vec!["a", "a"]);
        // First two occurrences (and their comments) survive; the third is gone.
        assert!(out.contains("# first"));
        assert!(out.contains("# second"));
        assert!(!out.contains("# third"));
    }

    #[test]
    fn seq_duplicate_with_reorder_no_spurious_churn() {
        // old [a, b, a] -> new [a, a, b]: both a's are survivors, b moves last.
        let src = "---\nx:\n- a # one\n- b # two\n- a # three\n---\nbody\n";
        let out = set_property_in_text(src, "x", &seq(&["a", "a", "b"])).unwrap();
        assert_eq!(list_of(&out, "x"), vec!["a", "a", "b"]);
        assert!(out.contains("# one") && out.contains("# two") && out.contains("# three"));
    }

    #[test]
    fn seq_flow_reorder_stays_flow() {
        let src = "---\nx: [a, b, c]\n---\nbody\n";
        let out = set_property_in_text(src, "x", &seq(&["c", "a", "b"])).unwrap();
        assert!(out.contains("[c, a, b]"), "expected flow list: {out}");
    }

    #[test]
    fn seq_noop_is_byte_identical() {
        let out = set_property_in_text(CONTENTS, "contents", &seq(&["a", "c", "b"])).unwrap();
        assert_eq!(out, CONTENTS);
    }

    #[test]
    fn seq_falls_back_when_not_eligible() {
        // Empty old sequence -> whole-value replace path (still preserves the
        // unrelated comment), exercising the decline branch.
        let empty = "---\ntitle: T\n# c\nitems: []\n---\nbody\n";
        let out = set_property_in_text(empty, "items", &seq(&["x", "y"])).unwrap();
        assert!(out.contains("# c"));
        assert_eq!(list_of(&out, "items"), vec!["x", "y"]);

        // Clearing to empty -> whole-value replace (item ops can't hold []).
        let out2 = set_property_in_text(CONTENTS, "contents", &Value::Sequence(vec![])).unwrap();
        assert!(out2.contains("title: T"));
        assert!(
            parse(&out2)
                .unwrap()
                .frontmatter
                .get("contents")
                .unwrap()
                .as_sequence()
                .unwrap()
                .is_empty()
        );

        // Disjoint replacement (all old removed) -> whole-value replace.
        let out3 = set_property_in_text(CONTENTS, "contents", &seq(&["p", "q"])).unwrap();
        assert_eq!(list_of(&out3, "contents"), vec!["p", "q"]);

        // Key absent -> insert path.
        let out4 = set_property_in_text(CONTENTS, "newlist", &seq(&["z"])).unwrap();
        assert_eq!(list_of(&out4, "newlist"), vec!["z"]);
    }

    #[test]
    fn remove_property_drops_key_keeps_comment() {
        let out = remove_property_in_text(NOTE, "title").unwrap();
        assert!(!out.contains("title:"));
        assert!(out.contains("# keep this comment"));
        assert!(out.contains("prose"));
    }

    #[test]
    fn remove_absent_key_is_unchanged() {
        assert_eq!(remove_property_in_text(NOTE, "nope").unwrap(), NOTE);
        assert_eq!(
            remove_property_in_text("plain body", "x").unwrap(),
            "plain body"
        );
    }

    #[test]
    fn rename_property_preserves_value_and_returns_some() {
        let out = rename_property_in_text(NOTE, "title", "name")
            .unwrap()
            .unwrap();
        let parsed = parse(&out).unwrap();
        assert!(parsed.frontmatter.get("title").is_none());
        assert_eq!(
            parsed.frontmatter.get("name").unwrap().as_str(),
            Some("Hello")
        );
        assert!(out.contains("# keep this comment"));
    }

    #[test]
    fn rename_absent_or_no_frontmatter_returns_none() {
        assert!(
            rename_property_in_text(NOTE, "nope", "x")
                .unwrap()
                .is_none()
        );
        assert!(
            rename_property_in_text("plain", "a", "b")
                .unwrap()
                .is_none()
        );
    }
}
