---
title: bookmatter
description: Order-preserving, round-trip metadata parser for plain text
author: adammharris
part_of: '[README](/crates/README.md)'
---

# bookmatter

A small, focused Rust crate for parsing **and** writing the metadata sections
of a plain text file.

## Quick example

```rust,ignore
use bookmatter::{parse, serialize, set_property, YamlValue};

let original = "---\ntitle: Hello\ntags: [a, b]\n---\n\nBody text.\n";

let mut parsed = parse(original)?;
set_property(&mut parsed.frontmatter, "draft", YamlValue::Bool(true));

let new_content = serialize(&parsed.frontmatter, &parsed.body)?;
assert!(new_content.contains("draft: true"));
```

## API surface

- `parse(&str) -> Result<ParsedFile>` — strict; fails when no frontmatter is present
- `parse_or_empty(&str) -> Result<ParsedFile>` — permissive; returns empty frontmatter when absent
- `serialize(&YamlMapping, body) -> Result<String>` — write a parsed file back out
- `parse_typed::<T>(&str)` / `serialize_typed(&T)` — typed (Serde) round trip
- `extract_yaml(&str)` / `extract_body(&str)` — slice without parsing
- `replace_body(&str, &str)` — body replacement that preserves the raw frontmatter byte-for-byte
- `get_property` / `set_property` / `remove_property` / `get_string` / `get_string_array`
- `sort_alphabetically` / `sort_by_pattern` — reorder keys

`YamlValue` is a small enum (`Null`, `Bool`, `Int`, `Float`, `String`, `Sequence`, `Mapping`)
with `From` impls for common Rust types and `serde_json::Value`, plus
`as_str` / `as_i64` / `as_mapping` / `as_mapping_mut` / `is_*` accessors.

## Filesystem agnostic

`bookmatter` does not touch the filesystem. Bring your own IO.

## Status

Extracted from [`diaryx_core`](../diaryx_core/README.md) where it has been
in production use since 2024.
