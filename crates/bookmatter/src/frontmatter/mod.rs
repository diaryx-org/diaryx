//! Frontmatter — metadata at the head of a markdown file, delimited by a pair
//! of fences (e.g. `---` for YAML, `+++` for TOML).
//!
//! Each format lives in its own submodule because the delimiter and parser are
//! jointly determined by the format. Only YAML is implemented today; future
//! formats (TOML, JSON) will live as sibling modules.

pub mod yaml;
