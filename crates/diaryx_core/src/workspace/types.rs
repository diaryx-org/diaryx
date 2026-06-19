//! Workspace data types.
//!
//! This module contains the core data types for workspace operations:
//! - `IndexFrontmatter` - Parsed frontmatter for workspace files
//! - `IndexFile` - A parsed file with frontmatter and body
//! - `TreeNode` - A node in the workspace tree for display

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use crate::yaml;

use crate::link_parser::{self, LinkFormat};

/// Normalize a path by resolving `.` and `..` components without filesystem access.
/// This is necessary for web/WASM where the virtual filesystem doesn't handle `..` in paths.
fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = Vec::new();

    for component in path.components() {
        match component {
            Component::ParentDir => {
                // Pop the last component if possible (handle ..)
                if !normalized.is_empty()
                    && !matches!(normalized.last(), Some(Component::ParentDir))
                {
                    normalized.pop();
                } else {
                    // Can't go up further, keep the ..
                    normalized.push(component);
                }
            }
            Component::CurDir => {
                // Skip . components
            }
            _ => {
                normalized.push(component);
            }
        }
    }

    normalized.iter().collect()
}

/// Pull the string-shaped elements out of a YAML sequence, coercing integers
/// via `i64::to_string` (the integer `Display` path, which — unlike float/bool
/// `to_string` — does not pull the Dragon4 float formatter into WASM). Floats,
/// bools, and nested shapes are dropped.
fn seq_strings(seq: Vec<yaml::Value>) -> Vec<String> {
    seq.into_iter()
        .filter_map(|v| match v {
            yaml::Value::String(s) => Some(s),
            yaml::Value::Int(n) => Some(n.to_string()),
            _ => None,
        })
        .collect()
}

/// Coerce a value that should be a string, tolerating a single-element string
/// array. The serde-free port of the former `deserialize_string_lenient`:
///
/// - String: returned as-is
/// - Array: takes the first string-shaped element (skipping the rest)
/// - Integer: coerced via `i64::to_string` (no `f64` formatting pulled in)
/// - Anything else (null, float, bool, mapping): treated as absent (`None`)
///
/// Non-string scalars were previously stringified via `to_string()`, which in
/// the float/bool arms pulled in `core::fmt::float` + the Dragon4 formatter
/// (~11 KB of WASM). Frontmatter titles/descriptions/etc. are always written as
/// strings by our own code, so permissive float/bool coercion is not worth the
/// binary-size cost.
fn coerce_string_lenient(value: yaml::Value) -> Option<String> {
    match value {
        yaml::Value::String(s) => Some(s),
        yaml::Value::Int(n) => Some(n.to_string()),
        yaml::Value::Sequence(seq) => seq.into_iter().find_map(|item| match item {
            yaml::Value::String(s) => Some(s),
            yaml::Value::Int(n) => Some(n.to_string()),
            _ => None,
        }),
        _ => None,
    }
}

/// Coerce a value that should be a `Vec<String>`, tolerating a bare string or
/// integer. The serde-free port of the former `deserialize_vec_string_lenient`:
///
/// - Array of strings (with optional integer elements): returned as-is
/// - Bare string: wrapped in a single-element vec
/// - Bare integer: wrapped as `[stringified]`
/// - Anything else (null, float, bool, mapping): treated as absent
fn coerce_vec_string_lenient(value: yaml::Value) -> Option<Vec<String>> {
    match value {
        yaml::Value::String(s) => Some(vec![s]),
        yaml::Value::Int(n) => Some(vec![n.to_string()]),
        yaml::Value::Sequence(seq) => Some(seq_strings(seq)),
        _ => None,
    }
}

/// Coerce a value that should be a list (`contents` / `attachments` /
/// `exclude`). Only a sequence yields `Some` — unlike [`coerce_vec_string_lenient`],
/// a bare scalar is *not* promoted to a one-element list, matching how these
/// structural fields parsed when they were plain `Option<Vec<String>>`. The
/// presence of an (even empty) list is what marks a file as an index, so the
/// distinction is load-bearing.
fn coerce_string_seq(value: yaml::Value) -> Option<Vec<String>> {
    match value {
        yaml::Value::Sequence(seq) => Some(seq_strings(seq)),
        _ => None,
    }
}

/// Represents an index file's frontmatter.
///
/// Parsing is serde-free: build with [`IndexFrontmatter::from_yaml_str`], which
/// walks `fig`'s native value tree rather than deriving `Deserialize`. The
/// `Serialize` derive is retained because [`IndexFile`] serializes (e.g. for the
/// CLI/JSON tree output); `skip_serializing_if` mirrors the previous output.
#[derive(Debug, Clone, Default, fig::ToValue)]
pub struct IndexFrontmatter {
    /// Display name for this index
    pub title: Option<String>,

    /// Canonical self-link for this file.
    /// When present, this should resolve back to the file itself.
    pub link: Option<String>,

    /// Description of this area
    pub description: Option<String>,

    /// List of paths to child index files (relative to this file)
    /// None means the key was absent; Some(vec) means it was present (even if empty)
    #[fig(skip_serializing_if = "Option::is_none")]
    pub contents: Option<Vec<String>>,

    /// Explicit outbound links declared by this file.
    #[fig(skip_serializing_if = "Option::is_none")]
    pub links: Option<Vec<String>>,

    /// Explicit backlinks from files whose `links` reference this file.
    #[fig(skip_serializing_if = "Option::is_none")]
    pub link_of: Option<Vec<String>>,

    /// Path to parent index file (relative to this file)
    /// If absent, this is a root index (workspace root)
    pub part_of: Option<String>,

    /// Audience groups that can see this file and its contents.
    /// If absent, inherits from parent; if at root with no audience and no
    /// `default_audience` in workspace config, the entry is private (excluded
    /// from exports). When `default_audience` is set, unconstrained entries
    /// are treated as belonging to that audience tag.
    #[fig(skip_serializing_if = "Option::is_none")]
    pub audience: Option<Vec<String>>,

    /// List of paths to attachment files (images, documents, etc.) relative to this file.
    /// Attachments declared here are available to this entry and all children.
    /// These values point to attachment notes (markdown files), whose singular
    /// `attachment` property points to the actual binary asset.
    #[fig(skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<String>>,

    /// Singular link to the binary asset represented by this attachment note.
    pub attachment: Option<String>,

    /// Reverse links from entries whose `attachments` reference this note.
    #[fig(skip_serializing_if = "Option::is_none")]
    pub attachment_of: Option<Vec<String>>,

    /// Glob patterns for files to exclude from orphan validation.
    /// Files matching these patterns won't trigger OrphanBinaryFile warnings.
    /// Example: `["*.lock", "*.toml", "build/*"]`
    #[fig(skip_serializing_if = "Option::is_none")]
    pub exclude: Option<Vec<String>>,

    // NOTE: `plugins` is intentionally NOT a typed field here. It is a workspace
    // config field (see `Workspace::WORKSPACE_CONFIG_FIELDS`) that lives in the
    // linked settings file, and it flows through `extra` like every other config
    // field so the config machinery (collect/migrate/get_workspace_config) can
    // treat it uniformly. The permission layer deserializes it into
    // `HashMap<String, PluginConfig>` where it actually needs the typed shape.
    /// Additional frontmatter properties
    #[fig(flatten)]
    pub extra: HashMap<String, yaml::Value>,
}

impl IndexFrontmatter {
    /// Parse index frontmatter from a YAML string without serde.
    ///
    /// The serde-free replacement for `yaml::from_str::<IndexFrontmatter>`: it
    /// parses via `fig`'s native parser ([`yaml::parse_value`]) and walks the
    /// resulting [`yaml::Value`] tree, applying the same lenient scalar/array
    /// coercions the old `deserialize_with` hooks did. Keys not matched by a
    /// named field land in `extra`, exactly like `#[serde(flatten)]`.
    pub fn from_yaml_str(s: &str) -> std::result::Result<Self, yaml::Error> {
        Ok(Self::from_value(yaml::parse_value(s)?))
    }

    /// Build from an already-parsed YAML value. A non-mapping top level yields
    /// the default (empty) frontmatter — matching serde, for which no named
    /// fields would be present.
    fn from_value(value: yaml::Value) -> Self {
        let mut map = match value {
            yaml::Value::Mapping(m) => m,
            _ => return Self::default(),
        };

        // Each named field is removed from the map so whatever is left flows
        // into `extra`, the way `#[serde(flatten)]` collected unknown keys.
        IndexFrontmatter {
            title: map.shift_remove("title").and_then(coerce_string_lenient),
            link: map.shift_remove("link").and_then(coerce_string_lenient),
            description: map
                .shift_remove("description")
                .and_then(coerce_string_lenient),
            contents: map.shift_remove("contents").and_then(coerce_string_seq),
            links: map
                .shift_remove("links")
                .and_then(coerce_vec_string_lenient),
            link_of: map
                .shift_remove("link_of")
                .and_then(coerce_vec_string_lenient),
            part_of: map.shift_remove("part_of").and_then(coerce_string_lenient),
            audience: map
                .shift_remove("audience")
                .and_then(coerce_vec_string_lenient),
            attachments: map.shift_remove("attachments").and_then(coerce_string_seq),
            attachment: map
                .shift_remove("attachment")
                .and_then(coerce_string_lenient),
            attachment_of: map
                .shift_remove("attachment_of")
                .and_then(coerce_vec_string_lenient),
            exclude: map.shift_remove("exclude").and_then(coerce_string_seq),
            extra: map.into_iter().collect(),
        }
    }

    /// Returns true if this is a root index (has contents property but no part_of)
    pub fn is_root(&self) -> bool {
        self.contents.is_some() && self.part_of.is_none()
    }

    /// Returns true if this is an index file (has contents property, even if empty)
    pub fn is_index(&self) -> bool {
        self.contents.is_some()
    }

    /// Get contents as a slice, or empty slice if absent
    pub fn contents_list(&self) -> &[String] {
        self.contents.as_deref().unwrap_or(&[])
    }

    /// Get links as a slice, or empty slice if absent
    pub fn links_list(&self) -> &[String] {
        self.links.as_deref().unwrap_or(&[])
    }

    /// Get backlinks as a slice, or empty slice if absent
    pub fn link_of_list(&self) -> &[String] {
        self.link_of.as_deref().unwrap_or(&[])
    }

    /// Get display name
    pub fn display_name(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Get attachments as a slice, or empty slice if absent
    pub fn attachments_list(&self) -> &[String] {
        self.attachments.as_deref().unwrap_or(&[])
    }

    /// Get reverse attachment links as a slice, or empty slice if absent.
    pub fn attachment_of_list(&self) -> &[String] {
        self.attachment_of.as_deref().unwrap_or(&[])
    }

    /// Returns true if this file has attachments
    pub fn has_attachments(&self) -> bool {
        self.attachments.as_ref().is_some_and(|a| !a.is_empty())
    }

    /// Get exclude patterns as a slice, or empty slice if absent
    pub fn exclude_list(&self) -> &[String] {
        self.exclude.as_deref().unwrap_or(&[])
    }

    /// Check if this file is visible to a given audience group.
    /// Returns None if audience should be inherited from parent (no explicit audience set).
    pub fn is_visible_to(&self, audience_group: &str) -> Option<bool> {
        // If no audience specified, inherit from parent
        let audience = self.audience.as_ref()?;

        // Check if the requested audience is in the list
        Some(
            audience
                .iter()
                .any(|a| a.trim().eq_ignore_ascii_case(audience_group.trim())),
        )
    }
}

/// Represents a parsed index file
#[derive(Debug, Clone, fig::ToValue)]
pub struct IndexFile {
    /// Path to the index file
    pub path: PathBuf,

    /// Parsed frontmatter
    pub frontmatter: IndexFrontmatter,

    /// Body content (after frontmatter)
    pub body: String,

    /// Link format hint for resolving ambiguous paths.
    /// When set to Some(LinkFormat::PlainCanonical), ambiguous paths like "Folder/file.md"
    /// are resolved relative to workspace root instead of relative to current file.
    #[fig(skip)]
    pub link_format_hint: Option<LinkFormat>,
}

impl IndexFile {
    /// Returns the directory containing this index file
    pub fn directory(&self) -> Option<&Path> {
        self.path.parent()
    }

    /// Resolve a path reference from this index's location.
    ///
    /// Handles multiple formats:
    /// - Markdown links: `[Title](/path/file.md)` or `[Title](../file.md)`
    /// - Plain paths with `/` prefix (workspace-root): `/path/file.md`
    /// - Plain relative paths: `../file.md` or `./file.md`
    /// - Plain ambiguous paths: `path/file.md` (treated based on link_format_hint)
    ///
    /// For ambiguous paths (no `/` prefix or `../`):
    /// - If `link_format_hint` is `Some(PlainCanonical)`, resolves as workspace-root
    /// - Otherwise, resolves relative to current file's directory (legacy behavior)
    ///
    /// Returns an absolute path resolved against this index file's location.
    /// The path is normalized to handle `..` and `.` components,
    /// which is necessary for web/WASM where the virtual filesystem
    /// doesn't automatically resolve these.
    pub fn resolve_path(&self, path_ref: &str) -> PathBuf {
        // Parse the link to extract the actual path and determine type
        let parsed = link_parser::parse_link(path_ref);

        match parsed.path_type {
            link_parser::PathType::WorkspaceRoot => {
                // Workspace-root paths are already canonical (workspace-relative).
                // Return as PathBuf directly - callers operate relative to workspace root.
                normalize_path(Path::new(&parsed.path))
            }
            link_parser::PathType::Relative => {
                // Explicit relative paths always resolve relative to current file
                let dir = self.directory().unwrap_or_else(|| std::path::Path::new(""));
                normalize_path(&dir.join(&parsed.path))
            }
            link_parser::PathType::Ambiguous => {
                // PlainCanonical writes ambiguous plain paths as workspace-root references.
                // Honor that when the workspace config explicitly signals PlainCanonical.
                if self.link_format_hint == Some(LinkFormat::PlainCanonical) {
                    normalize_path(Path::new(&parsed.path))
                } else {
                    // Legacy/default behavior for ambiguous paths.
                    let dir = self.directory().unwrap_or_else(|| std::path::Path::new(""));
                    normalize_path(&dir.join(&parsed.path))
                }
            }
        }
    }
}

/// Node in the workspace tree (for display purposes)
#[derive(Debug, Clone, fig::ToValue, fig::FromValue)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct TreeNode {
    /// Title of index/root file (or filename if no title)
    pub name: String,
    /// Description attribute (if given)
    pub description: Option<String>,
    /// Path to index/root file
    pub path: PathBuf,
    /// Whether this node has a `contents` property (even if empty)
    #[fig(default)]
    pub is_index: bool,
    /// `contents` property list
    pub children: Vec<TreeNode>,
    /// Additional frontmatter properties for display (populated by --properties flag)
    #[cfg_attr(
        feature = "typescript",
        ts(type = "Record<string, string> | undefined")
    )]
    #[fig(default, skip_serializing_if = "HashMap::is_empty")]
    pub properties: HashMap<String, String>,
    /// Audience tags from frontmatter (empty if not set)
    #[fig(default, skip_serializing_if = "Vec::is_empty")]
    pub audience: Vec<String>,
}

/// Helper function to format a tree node for display
pub fn format_tree_node(node: &TreeNode, prefix: &str) -> String {
    let mut result = String::new();

    // Add the current node name
    result.push_str(&node.name);

    // Add description if present
    if let Some(ref desc) = node.description {
        result.push_str(" - ");
        result.push_str(desc);
    }

    // Add properties if present
    if !node.properties.is_empty() {
        let props: Vec<String> = node
            .properties
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        result.push_str(" [");
        result.push_str(&props.join(", "));
        result.push(']');
    }

    result.push('\n');

    // Add children
    let child_count = node.children.len();
    for (i, child) in node.children.iter().enumerate() {
        let is_last_child = i == child_count - 1;
        let connector = if is_last_child {
            "└── "
        } else {
            "├── "
        };
        let child_prefix = if is_last_child { "    " } else { "│   " };

        result.push_str(prefix);
        result.push_str(connector);
        result.push_str(&format_tree_node(
            child,
            &format!("{}{}", prefix, child_prefix),
        ));
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_index_file(path: &str, link_format_hint: Option<LinkFormat>) -> IndexFile {
        IndexFile {
            path: PathBuf::from(path),
            frontmatter: IndexFrontmatter::default(),
            body: String::new(),
            link_format_hint,
        }
    }

    #[test]
    fn test_resolve_path_workspace_root() {
        // Workspace-root paths (with /) should always resolve as-is
        let index = make_index_file("A/B/index.md", None);
        let resolved = index.resolve_path("/Folder/file.md");
        assert_eq!(resolved, PathBuf::from("Folder/file.md"));
    }

    #[test]
    fn test_resolve_path_relative() {
        // Relative paths (../) should always resolve relative to current file
        let index = make_index_file("A/B/index.md", None);
        let resolved = index.resolve_path("../sibling.md");
        assert_eq!(resolved, PathBuf::from("A/sibling.md"));
    }

    #[test]
    fn test_resolve_path_ambiguous_no_hint() {
        // Without hint, ambiguous paths resolve relative to current file
        let index = make_index_file("A/B/index.md", None);
        let resolved = index.resolve_path("Folder/file.md");
        assert_eq!(resolved, PathBuf::from("A/B/Folder/file.md"));
    }

    #[test]
    fn test_resolve_path_ambiguous_with_plain_canonical_hint() {
        // PlainCanonical hint resolves ambiguous paths as workspace-root.
        let index = make_index_file("A/B/index.md", Some(LinkFormat::PlainCanonical));
        let resolved = index.resolve_path("Folder/file.md");
        assert_eq!(resolved, PathBuf::from("Folder/file.md"));
    }

    #[test]
    fn test_resolve_path_ambiguous_with_markdown_root_hint() {
        // Ambiguous paths always resolve relative to current file for backwards compatibility.
        // The link format hint only affects how NEW links are WRITTEN.
        let index = make_index_file("A/B/index.md", Some(LinkFormat::MarkdownRoot));
        let resolved = index.resolve_path("Folder/file.md");
        assert_eq!(resolved, PathBuf::from("A/B/Folder/file.md"));
    }

    #[test]
    fn test_resolve_path_markdown_link_root() {
        // Markdown links with root path
        let index = make_index_file("A/B/index.md", None);
        let resolved = index.resolve_path("[Title](/Folder/file.md)");
        assert_eq!(resolved, PathBuf::from("Folder/file.md"));
    }

    #[test]
    fn test_resolve_path_markdown_link_relative() {
        // Markdown links with relative path
        let index = make_index_file("A/B/index.md", None);
        let resolved = index.resolve_path("[Title](../sibling.md)");
        assert_eq!(resolved, PathBuf::from("A/sibling.md"));
    }

    #[test]
    fn test_resolve_path_markdown_link_ambiguous_with_hint() {
        // PlainCanonical hint also applies to markdown links with ambiguous URLs.
        let index = make_index_file("A/B/index.md", Some(LinkFormat::PlainCanonical));
        let resolved = index.resolve_path("[Title](Folder/file.md)");
        assert_eq!(resolved, PathBuf::from("Folder/file.md"));
    }

    #[test]
    fn test_resolve_path_plain_canonical_real_world_case() {
        // PlainCanonical reads ambiguous paths as workspace-root.
        let index = make_index_file("Projects/Ideas/index.md", Some(LinkFormat::PlainCanonical));

        // Contents ref - resolves from workspace root
        let resolved = index.resolve_path("Daily/2025/01/01.md");
        assert_eq!(resolved, PathBuf::from("Daily/2025/01/01.md"));

        // Part_of ref - resolves from workspace root
        let resolved = index.resolve_path("Projects/index.md");
        assert_eq!(resolved, PathBuf::from("Projects/index.md"));
    }

    #[test]
    fn test_resolve_path_ambiguous_with_markdown_relative_hint() {
        // With MarkdownRelative hint, ambiguous paths resolve relative (legacy behavior)
        let index = make_index_file("A/B/index.md", Some(LinkFormat::MarkdownRelative));
        let resolved = index.resolve_path("Folder/file.md");
        assert_eq!(resolved, PathBuf::from("A/B/Folder/file.md"));
    }

    #[test]
    fn test_resolve_path_ambiguous_with_plain_relative_hint() {
        // With PlainRelative hint, ambiguous paths resolve relative (legacy behavior)
        let index = make_index_file("A/B/index.md", Some(LinkFormat::PlainRelative));
        let resolved = index.resolve_path("Folder/file.md");
        assert_eq!(resolved, PathBuf::from("A/B/Folder/file.md"));
    }

    #[test]
    fn test_resolve_path_explicit_relative_ignores_hint() {
        // Explicit relative paths (with ./ or ../) should always resolve relative,
        // even with PlainCanonical hint
        let index = make_index_file("A/B/index.md", Some(LinkFormat::PlainCanonical));

        let resolved = index.resolve_path("./sibling.md");
        assert_eq!(resolved, PathBuf::from("A/B/sibling.md"));

        let resolved = index.resolve_path("../parent.md");
        assert_eq!(resolved, PathBuf::from("A/parent.md"));
    }

    #[test]
    fn test_audience_bare_string_deserialized_as_vec() {
        // audience: private (bare string) should become vec!["private"]
        let yaml = "audience: private\n";
        let fm = IndexFrontmatter::from_yaml_str(yaml).unwrap();
        assert_eq!(fm.audience, Some(vec!["private".to_string()]));
    }

    #[test]
    fn test_audience_array_deserialized_normally() {
        let yaml = "audience:\n  - family\n  - private\n";
        let fm = IndexFrontmatter::from_yaml_str(yaml).unwrap();
        assert_eq!(
            fm.audience,
            Some(vec!["family".to_string(), "private".to_string()])
        );
    }

    #[test]
    fn from_yaml_str_collects_unknown_keys_into_extra() {
        // Named fields are typed; everything else flows into `extra`, exactly
        // like the old `#[serde(flatten)]`.
        let yaml = "title: Home\ncustom_key: hello\ncount: 3\n";
        let fm = IndexFrontmatter::from_yaml_str(yaml).unwrap();
        assert_eq!(fm.title.as_deref(), Some("Home"));
        assert_eq!(
            fm.extra.get("custom_key"),
            Some(&yaml::Value::String("hello".into()))
        );
        assert_eq!(fm.extra.get("count"), Some(&yaml::Value::Int(3)));
        // A typed field never leaks into `extra`.
        assert!(!fm.extra.contains_key("title"));
    }

    #[test]
    fn from_yaml_str_non_mapping_is_default() {
        // A scalar / sequence top level has no named fields — empty frontmatter.
        assert!(
            IndexFrontmatter::from_yaml_str("just a string\n")
                .unwrap()
                .extra
                .is_empty()
        );
        assert!(
            IndexFrontmatter::from_yaml_str("- a\n- b\n")
                .unwrap()
                .title
                .is_none()
        );
    }

    #[test]
    fn from_yaml_str_empty_contents_marks_index() {
        // An explicit empty list is `Some(vec![])` (file is an index); an absent
        // key is `None`. A bare scalar is NOT promoted to a one-item list.
        assert_eq!(
            IndexFrontmatter::from_yaml_str("contents: []\n")
                .unwrap()
                .contents,
            Some(vec![])
        );
        assert!(
            IndexFrontmatter::from_yaml_str("contents: []\n")
                .unwrap()
                .is_index()
        );
        assert_eq!(
            IndexFrontmatter::from_yaml_str("title: x\n")
                .unwrap()
                .contents,
            None
        );
        assert_eq!(
            IndexFrontmatter::from_yaml_str("contents: oops\n")
                .unwrap()
                .contents,
            None
        );
    }

    #[test]
    fn from_yaml_str_coerces_integer_title() {
        // `title: 2025` (a hand-written int) coerces to a string via the integer
        // Display path, without pulling the float formatter.
        let fm = IndexFrontmatter::from_yaml_str("title: 2025\n").unwrap();
        assert_eq!(fm.title.as_deref(), Some("2025"));
    }

    #[test]
    fn test_audience_helpers_trim_values() {
        let fm = IndexFrontmatter {
            audience: Some(vec![
                " family ".to_string(),
                " private ".to_string(),
                " ENGL212 ".to_string(),
            ]),
            ..Default::default()
        };

        // "private" is now just a regular audience tag, no special meaning
        assert_eq!(fm.is_visible_to("family"), Some(true));
        assert_eq!(fm.is_visible_to("private"), Some(true));
        assert_eq!(fm.is_visible_to("engl212"), Some(true));
        assert_eq!(fm.is_visible_to("unknown"), Some(false));

        let no_audience_fm = IndexFrontmatter {
            ..Default::default()
        };
        // No audience = inherit from parent (returns None)
        assert_eq!(no_audience_fm.is_visible_to("family"), None);
    }
}
