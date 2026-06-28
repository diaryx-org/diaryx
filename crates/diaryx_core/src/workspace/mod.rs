//! Workspace operations module.
//!
//! This module provides functionality for working with Diaryx workspaces:
//! - Tree traversal and building
//! - File operations (move, rename, delete)
//! - Index management (contents, part_of relationships)
//!
//! # Module Structure
//!
//! - `types` - Core data types (IndexFrontmatter, IndexFile, TreeNode)
//!
//! # Async-first Design
//!
//! This module uses `AsyncFileSystem` for all filesystem operations.
//! For synchronous contexts (CLI, tests), wrap a sync filesystem with
//! `SyncToAsyncFs` and use `futures_lite::future::block_on()`.

mod config;
mod entry;
#[cfg(test)]
mod tests;
mod tree;
mod tree_selection;
mod types;

// Re-export types for backwards compatibility
pub use tree_selection::*;
pub use types::{IndexFile, IndexFrontmatter, TreeNode, format_tree_node};

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::yaml;

use crate::error::{DiaryxError, Result};
use crate::fs::AsyncFileSystem;
use crate::link_parser::{self, LinkFormat};
use crate::path_utils::normalize_sync_path;
use crate::utils::matches_glob_pattern;

/// How to generate filenames from entry titles.
#[derive(Debug, Clone, Default, PartialEq, Eq, fig::ToValue, fig::FromValue)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
#[fig(rename_all = "snake_case")]
#[cfg_attr(feature = "typescript", ts(rename_all = "snake_case"))]
pub enum FilenameStyle {
    /// Keep the title as-is, stripping only filesystem-illegal characters.
    #[default]
    Preserve,
    /// Lowercase, non-alphanumeric chars replaced with dashes.
    KebabCase,
    /// Lowercase, non-alphanumeric chars replaced with underscores.
    SnakeCase,
    /// Uppercase, non-alphanumeric chars replaced with underscores.
    ScreamingSnakeCase,
}

fn default_true() -> bool {
    true
}

/// A challenge gate that controls reader access to an audience's content.
///
/// Gates are stackable; an audience with no gates is public, and multiple
/// gates are evaluated with OR semantics by the site-proxy worker (any
/// satisfied gate grants access). This is the workspace-file shape — server
/// state (e.g. password hashes) lives separately and is keyed by gate kind.
///
/// Phase 1 variants are unit-style; future kinds (e.g. `ip_allowlist`,
/// `totp`) can be added without breaking older clients because unknown
/// variants are tolerated by the parser.
#[derive(Debug, Clone, PartialEq, Eq, fig::ToValue, fig::FromValue)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
#[fig(tag = "kind", rename_all = "snake_case")]
#[cfg_attr(feature = "typescript", ts(tag = "kind", rename_all = "snake_case"))]
pub enum Gate {
    /// Reader presents a signed magic-link token whose `audience` claim
    /// matches. The server signing key handles all validation.
    Link,
    /// Reader submits a password that Argon2-verifies against the
    /// server-stored hash. The hash is intentionally not in the file —
    /// it lives in the server's `namespace_audiences.gates` JSON.
    Password,
}

/// A labeled share channel the writer can invoke from the audience UI to
/// distribute the audience's URL. Channels are clipboard-mediated by design;
/// the writer is the one who actually presses send. Tagged so future kinds
/// (`discord_webhook`, etc.) can be added later.
#[derive(Debug, Clone, PartialEq, Eq, fig::ToValue, fig::FromValue)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
#[fig(tag = "kind", rename_all = "snake_case")]
#[cfg_attr(feature = "typescript", ts(tag = "kind", rename_all = "snake_case"))]
pub enum ShareAction {
    /// Pre-fill a `mailto:` URL with the recipient list (BCC), templated
    /// subject, and templated body. Templates support `{{title}}` and
    /// `{{url}}` placeholders, resolved by the UI when the button is
    /// pressed.
    Email {
        /// BCC list packed into the generated `mailto:` URL.
        #[fig(default)]
        recipients: Vec<String>,
        /// `{{title}}` / `{{url}}`-templated subject line. Falls back to a
        /// generic subject if absent.
        #[cfg_attr(feature = "typescript", ts(optional))]
        #[fig(default, skip_serializing_if = "Option::is_none")]
        subject_template: Option<String>,
        /// `{{title}}` / `{{url}}`-templated body text. Falls back to just
        /// the URL if absent.
        #[cfg_attr(feature = "typescript", ts(optional))]
        #[fig(default, skip_serializing_if = "Option::is_none")]
        body_template: Option<String>,
    },
    /// A labeled copy-to-clipboard button. The label is purely informational
    /// — it lets the writer name the channel ("For the group chat") so the
    /// UI shows it next to the URL.
    CopyLink {
        /// Optional label rendered next to the copy button. Falls back to
        /// "Copy link" when absent.
        #[cfg_attr(feature = "typescript", ts(optional))]
        #[fig(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
}

/// A workspace-declared audience with a set of access gates and labeled
/// share-channel shortcuts. The workspace file is the writer's source of
/// truth; the server's audience records mirror this list.
#[derive(Debug, Clone, PartialEq, Eq, fig::ToValue, fig::FromValue)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct AudienceDecl {
    /// Audience name. Must match the `audience:` tag used on individual
    /// entries to control visibility.
    pub name: String,
    /// Stackable challenge gates. Empty list = public.
    #[fig(default)]
    pub gates: Vec<Gate>,
    /// Labeled share channels surfaced in the audience UI. Empty list is
    /// fine — the UI always offers a generic copy-link affordance.
    #[fig(default)]
    pub share_actions: Vec<ShareAction>,
}

/// Publishing configuration for a workspace, stored under the top-level
/// `publish:` key in the workspace settings file.
///
/// This replaces the former `plugins."diaryx.publish".config` location (publish
/// is integrated into the app, not a plugin) and absorbs the previously
/// top-level `audiences` declaration. `default_audience` stays top-level because
/// it also governs local export visibility, not just publishing.
#[derive(Debug, Clone, Default, PartialEq, fig::ToValue, fig::FromValue)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct PublishSettings {
    /// Server namespace this workspace publishes to (server-assigned on first
    /// publish; persisted here so the binding travels with the workspace).
    #[cfg_attr(feature = "typescript", ts(optional))]
    #[fig(default, skip_serializing_if = "Option::is_none")]
    pub namespace_id: Option<String>,

    /// Claimed subdomain for the published site, if any.
    #[cfg_attr(feature = "typescript", ts(optional))]
    #[fig(default, skip_serializing_if = "Option::is_none")]
    pub subdomain: Option<String>,

    /// Declared audiences for selective sharing. When present, this is the
    /// authoritative list — publish syncs it to the server (creating, updating,
    /// or removing audience records to match). When absent, publish falls back
    /// to the legacy `audience_states` map for backward compatibility.
    #[cfg_attr(feature = "typescript", ts(optional))]
    #[fig(default, skip_serializing_if = "Option::is_none")]
    pub audiences: Option<Vec<AudienceDecl>>,

    /// True once any pre-existing `audience_states` have been imported into the
    /// `audiences` declaration. While false, publish is dual-read (file OR
    /// legacy) and non-destructive; once true the file is strict-truth.
    #[cfg_attr(feature = "typescript", ts(optional))]
    #[fig(default, skip_serializing_if = "Option::is_none")]
    pub audiences_migrated: Option<bool>,

    /// Legacy per-audience publish state map (pre-`audiences`). Kept as an opaque
    /// value so the config layer stays agnostic to its shape; superseded by
    /// `audiences` for migrated workspaces.
    #[cfg_attr(feature = "typescript", ts(optional))]
    #[fig(default, skip_serializing_if = "Option::is_none")]
    pub audience_states: Option<yaml::Value>,

    /// Legacy list of public audience names (pre-`audiences`).
    #[cfg_attr(feature = "typescript", ts(optional))]
    #[fig(default, skip_serializing_if = "Option::is_none")]
    pub public_audiences: Option<Vec<String>>,
}

/// Workspace-level configuration stored in the root index file's frontmatter.
///
/// This allows workspace settings to live with the data (local-first philosophy)
/// rather than in separate config files.
#[derive(Debug, Clone, Default, fig::ToValue, fig::FromValue)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct WorkspaceConfig {
    /// Format for `part_of`, `contents`, and `attachments` links.
    /// Defaults to MarkdownRoot if not specified.
    #[fig(default)]
    pub link_format: LinkFormat,

    /// Link to the default template entry for new files (in link_format style).
    /// If absent, uses the built-in "note" template.
    #[cfg_attr(feature = "typescript", ts(optional))]
    #[fig(default, skip_serializing_if = "Option::is_none")]
    pub default_template: Option<String>,

    /// When true, setting the `title` frontmatter property also updates the first H1 heading.
    /// Unidirectional: title → heading only.
    #[fig(default)]
    pub sync_title_to_heading: bool,

    /// When true, saving content automatically updates the `updated` timestamp.
    #[fig(default = "default_true")]
    pub auto_update_timestamp: bool,

    /// When true, changing the title automatically renames the file.
    #[fig(default = "default_true")]
    pub auto_rename_to_title: bool,

    /// How to generate filenames from entry titles.
    #[fig(default)]
    pub filename_style: FilenameStyle,

    /// Audience tag assigned to entries with no explicit or inherited audience.
    /// Unset = private (excluded from exports). Entries that have this tag via
    /// the default are included in audience-filtered exports for that tag.
    #[cfg_attr(feature = "typescript", ts(optional))]
    #[fig(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "public_audience"
    )]
    pub default_audience: Option<String>,

    /// Folder used by the Daily plugin for date-based entries.
    #[cfg_attr(feature = "typescript", ts(optional))]
    #[fig(default, skip_serializing_if = "Option::is_none")]
    pub daily_entry_folder: Option<String>,

    /// When true, show files not linked in the workspace hierarchy.
    #[cfg_attr(feature = "typescript", ts(optional))]
    #[fig(default, skip_serializing_if = "Option::is_none")]
    pub show_unlinked_files: Option<bool>,

    /// When true, show hidden (dot-prefixed) files in the tree.
    #[cfg_attr(feature = "typescript", ts(optional))]
    #[fig(default, skip_serializing_if = "Option::is_none")]
    pub show_hidden_files: Option<bool>,

    /// Theme mode preference for this workspace: "light", "dark", or "system".
    #[cfg_attr(feature = "typescript", ts(optional))]
    #[fig(default, skip_serializing_if = "Option::is_none")]
    pub theme_mode: Option<String>,

    /// Active workspace theme preset ID.
    #[cfg_attr(feature = "typescript", ts(optional))]
    #[fig(default, skip_serializing_if = "Option::is_none")]
    pub theme_preset: Option<String>,

    /// Accent hue override applied to the active theme preset.
    #[cfg_attr(feature = "typescript", ts(optional))]
    #[fig(default, skip_serializing_if = "Option::is_none")]
    pub theme_accent_hue: Option<f64>,

    /// Map of audience name → Tailwind color class (e.g., "family" → "bg-indigo-500").
    #[cfg_attr(feature = "typescript", ts(optional))]
    #[fig(default, skip_serializing_if = "Option::is_none")]
    pub audience_colors: Option<std::collections::HashMap<String, String>>,

    /// List of plugin IDs that are explicitly disabled in this workspace.
    /// Plugins not listed here are enabled by default.
    #[cfg_attr(feature = "typescript", ts(optional))]
    #[fig(default, skip_serializing_if = "Option::is_none")]
    pub disabled_plugins: Option<Vec<String>>,

    /// Per-plugin workspace configuration keyed by plugin ID — install records
    /// and granted permissions (`plugins.<id>.permissions`). Stored as a raw
    /// value (the same nested shape consumers already type as
    /// `Record<string, PluginConfig>`) so the workspace config layer stays
    /// agnostic to the permission schema, which lives in
    /// `crate::plugin::permissions`.
    #[cfg_attr(feature = "typescript", ts(optional))]
    #[fig(default, skip_serializing_if = "Option::is_none")]
    pub plugins: Option<yaml::Value>,

    /// Publishing configuration: namespace binding, subdomain, and the declared
    /// `audiences`. Replaces the former `plugins."diaryx.publish".config` and the
    /// top-level `audiences`/`audiences_migrated` keys (both relocated here by the
    /// eager migration on workspace open).
    #[cfg_attr(feature = "typescript", ts(optional))]
    #[fig(default, skip_serializing_if = "Option::is_none")]
    pub publish: Option<PublishSettings>,
}

#[derive(Default)]
struct FilesystemTreeTrace {
    explored_dirs: BTreeSet<String>,
    pruned_excluded_dirs: BTreeSet<String>,
    pruned_skip_dirs: BTreeSet<String>,
}

impl FilesystemTreeTrace {
    fn record_explored(&mut self, path: &Path) {
        self.explored_dirs
            .insert(path.to_string_lossy().to_string());
    }

    fn record_pruned_excluded(&mut self, path: &Path) {
        self.pruned_excluded_dirs
            .insert(path.to_string_lossy().to_string());
    }

    fn record_pruned_skip(&mut self, path: &Path) {
        self.pruned_skip_dirs
            .insert(path.to_string_lossy().to_string());
    }
}

/// Workspace operations (async-first).
///
/// All methods are async and use `AsyncFileSystem` for filesystem access.
pub struct Workspace<FS: AsyncFileSystem> {
    fs: FS,
    /// The workspace root directory path (for computing canonical paths)
    root_path: Option<PathBuf>,
    /// Link format for `part_of`, `contents`, and `attachments` properties
    link_format: LinkFormat,
}

impl<FS: AsyncFileSystem> Workspace<FS> {
    /// Create a new workspace without link formatting (legacy mode).
    /// Links will be written as relative paths.
    pub fn new(fs: FS) -> Self {
        Self {
            fs,
            root_path: None,
            link_format: LinkFormat::PlainRelative,
        }
    }

    /// Create a new workspace with link formatting enabled.
    ///
    /// # Arguments
    /// * `fs` - The filesystem to use
    /// * `root_path` - The workspace root directory (for computing canonical paths)
    /// * `link_format` - How to format `part_of`, `contents`, and `attachments` links
    pub fn with_link_format(fs: FS, root_path: PathBuf, link_format: LinkFormat) -> Self {
        Self {
            fs,
            root_path: Some(root_path),
            link_format,
        }
    }

    /// Get a reference to the underlying filesystem
    pub fn fs_ref(&self) -> &FS {
        &self.fs
    }

    /// Get the canonical path (workspace-relative) for a filesystem path.
    /// Returns the path as-is if no root_path is configured.
    ///
    /// The canonical path:
    /// - Has no leading `/` or `./`
    /// - Uses forward slashes
    /// - Is relative to the workspace root
    fn get_canonical_path(&self, path: &Path) -> String {
        let raw = if let Some(ref root) = self.root_path {
            // Strip the root path prefix to get workspace-relative path
            path.strip_prefix(root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/") // Normalize to forward slashes
        } else {
            path.to_string_lossy().replace('\\', "/")
        };

        normalize_sync_path(&raw)
    }

    /// Resolve a title for a canonical path by reading the file's frontmatter.
    /// Falls back to a formatted filename if the file can't be read.
    async fn resolve_title(&self, canonical_path: &str) -> String {
        if let Some(ref root) = self.root_path {
            let full_path = root.join(canonical_path);
            if let Ok(content) = self.fs.read_to_string(&full_path).await {
                // Resolve the title through the shared frontmatter parser so
                // inline YAML comments, quoting, and escapes are handled
                // correctly. Hand-rolling this (an earlier `title:` line scan)
                // leaked `# comment` text into the title — and from there into
                // every link that references this file.
                if let Ok(parsed) = crate::frontmatter::parse_or_empty(&content)
                    && let Some(title) =
                        crate::frontmatter::get_string(&parsed.frontmatter, "title")
                    && !title.is_empty()
                {
                    return title.to_string();
                }
            }
        }
        // Fallback: convert filename to title
        link_parser::path_to_title(canonical_path)
    }

    /// Resolve a file's `part_of` frontmatter to an absolute parent index path.
    ///
    /// Reads the `part_of` property from `file_path`, parses the link, and
    /// resolves it to an absolute path. Falls back to `find_any_index_in_dir`
    /// on `fallback_dir` when `part_of` is absent or unresolvable.
    async fn resolve_part_of_to_path(
        &self,
        file_path: &Path,
        fallback_dir: &Path,
    ) -> Option<PathBuf> {
        self.resolve_part_of_from_dir(file_path, file_path.parent(), fallback_dir)
            .await
    }

    /// Like `resolve_part_of_to_path`, but resolves relative `part_of` links
    /// from `resolve_dir` instead of from `file_path.parent()`.
    ///
    /// This is needed by `sync_move_metadata` where the file has already been
    /// moved to a new location but its `part_of` still contains a relative link
    /// written for the old location.
    async fn resolve_part_of_from_dir(
        &self,
        file_path: &Path,
        resolve_dir: Option<&Path>,
        fallback_dir: &Path,
    ) -> Option<PathBuf> {
        use crate::path_utils::normalize_path;

        if let Ok(Some(yaml::Value::String(part_of))) =
            self.get_frontmatter_property(file_path, "part_of").await
        {
            let dir = resolve_dir.unwrap_or_else(|| Path::new(""));
            let parsed = link_parser::parse_link(&part_of);
            let resolved = match parsed.path_type {
                link_parser::PathType::WorkspaceRoot => {
                    if let Some(ref root) = self.root_path {
                        normalize_path(&root.join(&parsed.path))
                    } else {
                        PathBuf::from(&parsed.path)
                    }
                }
                link_parser::PathType::Relative | link_parser::PathType::Ambiguous => {
                    normalize_path(&dir.join(&parsed.path))
                }
            };
            return Some(resolved);
        }

        // Fallback: search for an index in the given directory
        self.find_any_index_in_dir(fallback_dir)
            .await
            .ok()
            .flatten()
    }

    /// Collect direct child file paths from an index's `contents` entries.
    ///
    /// Paths are resolved using the same link parsing rules as other workspace
    /// operations, with `self.link_format` used as the hint for ambiguous paths.
    async fn collect_index_content_children(&self, index_path: &Path) -> Vec<PathBuf> {
        let mut children = Vec::new();
        let mut seen = HashSet::new();

        let index = match self.parse_index(index_path).await {
            Ok(index) => index,
            Err(e) => {
                log::warn!(
                    "collect_index_content_children: failed to parse '{}': {}",
                    index_path.display(),
                    e
                );
                return children;
            }
        };

        let index_canonical = self.get_canonical_path(index_path);
        let index_canonical_path = Path::new(&index_canonical);

        for raw_child in index.frontmatter.contents_list() {
            let parsed = link_parser::parse_link(raw_child);
            let child_canonical = link_parser::to_canonical_with_link_format(
                &parsed,
                index_canonical_path,
                Some(self.link_format),
            );

            if !seen.insert(child_canonical.clone()) {
                continue;
            }

            let child_path = if let Some(root) = &self.root_path {
                root.join(&child_canonical)
            } else {
                PathBuf::from(&child_canonical)
            };
            children.push(child_path);
        }

        children
    }

    /// Format a link for frontmatter based on configured link format.
    ///
    /// # Arguments
    /// * `target_canonical` - The canonical path of the target file
    /// * `from_canonical` - The canonical path of the file containing the link
    #[allow(dead_code)]
    async fn format_link(&self, target_canonical: &str, from_canonical: &str) -> String {
        let title = self.resolve_title(target_canonical).await;
        link_parser::format_link_with_format(
            target_canonical,
            &title,
            self.link_format,
            from_canonical,
        )
    }

    /// Format a link synchronously (when title is already known).
    fn format_link_sync(
        &self,
        target_canonical: &str,
        title: &str,
        from_canonical: &str,
    ) -> String {
        link_parser::format_link_with_format(
            target_canonical,
            title,
            self.link_format,
            from_canonical,
        )
    }

    /// Parse a markdown file and extract index frontmatter
    pub async fn parse_index(&self, path: &Path) -> Result<IndexFile> {
        let content = self
            .fs
            .read_to_string(path)
            .await
            .map_err(|e| DiaryxError::FileRead {
                path: path.to_path_buf(),
                source: e,
            })?;

        let (frontmatter_str, body) = crate::frontmatter::split(&content)
            .ok_or_else(|| DiaryxError::NoFrontmatter(path.to_path_buf()))?;

        let frontmatter: IndexFrontmatter = IndexFrontmatter::from_yaml_str(frontmatter_str)
            .map_err(|e| DiaryxError::YamlParse {
                path: path.to_path_buf(),
                message: e.to_string(),
            })?;

        Ok(IndexFile {
            path: path.to_path_buf(),
            frontmatter,
            body: body.to_string(),
            link_format_hint: None,
        })
    }

    /// Parse a markdown file and extract index frontmatter, with a link format hint.
    ///
    /// This variant of `parse_index` allows setting the `link_format_hint` field
    /// which affects how ambiguous paths (like `Folder/file.md`) are resolved.
    /// When `link_format` is `Some(PlainCanonical)`, ambiguous paths are resolved
    /// as workspace-root paths instead of relative paths.
    pub async fn parse_index_with_hint(
        &self,
        path: &Path,
        link_format: Option<LinkFormat>,
    ) -> Result<IndexFile> {
        let mut index = self.parse_index(path).await?;
        index.link_format_hint = link_format;
        Ok(index)
    }

    /// Parse a markdown file with caching. Returns a clone from the cache on hit,
    /// or parses and inserts on miss. `None` is cached for non-parseable files.
    async fn parse_index_cached(
        &self,
        path: &Path,
        cache: &mut HashMap<PathBuf, Option<IndexFile>>,
    ) -> Result<IndexFile> {
        let key = path.to_path_buf();
        if let Some(cached) = cache.get(&key) {
            return cached.clone().ok_or(DiaryxError::NoFrontmatter(key));
        }
        match self.parse_index(path).await {
            Ok(index) => {
                cache.insert(key, Some(index.clone()));
                Ok(index)
            }
            Err(e) => {
                cache.insert(key, None);
                Err(e)
            }
        }
    }

    /// Check if a file is an index file (has contents property)
    pub async fn is_index_file(&self, path: &Path) -> bool {
        if path.extension().is_none_or(|ext| ext != "md") {
            return false;
        }

        self.parse_index(path)
            .await
            .map(|idx| idx.frontmatter.is_index())
            .unwrap_or(false)
    }

    /// Check if a file is a root index (has contents but no part_of)
    pub async fn is_root_index(&self, path: &Path) -> bool {
        self.parse_index(path)
            .await
            .map(|idx| idx.frontmatter.is_root())
            .unwrap_or(false)
    }

    /// Find a root index in the given directory
    pub async fn find_root_index_in_dir(&self, dir: &Path) -> Result<Option<PathBuf>> {
        let md_files = self
            .fs
            .read_dir(dir)
            .await
            .map(|entries| {
                entries
                    .into_iter()
                    .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
                    .map(|e| e.path().to_path_buf())
                    .collect::<Vec<_>>()
            })
            .map_err(|e| DiaryxError::FileRead {
                path: dir.to_path_buf(),
                source: e,
            })?;

        for file in md_files {
            if self.is_root_index(&file).await {
                return Ok(Some(file));
            }
        }

        Ok(None)
    }

    /// Find any index file in the given directory (has `contents` property)
    /// Prefers root indexes over non-root indexes
    pub async fn find_any_index_in_dir(&self, dir: &Path) -> Result<Option<PathBuf>> {
        let md_files = self
            .fs
            .read_dir(dir)
            .await
            .map(|entries| {
                entries
                    .into_iter()
                    .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
                    .map(|e| e.path().to_path_buf())
                    .collect::<Vec<_>>()
            })
            .map_err(|e| DiaryxError::FileRead {
                path: dir.to_path_buf(),
                source: e,
            })?;

        let mut found_index: Option<PathBuf> = None;

        for file in md_files {
            if let Ok(index) = self.parse_index(&file).await
                && index.frontmatter.is_index()
            {
                // Prefer root index if found
                if index.frontmatter.is_root() {
                    return Ok(Some(file));
                }
                // Otherwise remember the first index we find
                if found_index.is_none() {
                    found_index = Some(file);
                }
            }
        }

        Ok(found_index)
    }

    /// Cached variant of `find_any_index_in_dir` for use during tree builds.
    async fn find_any_index_in_dir_cached(
        &self,
        dir: &Path,
        cache: &mut HashMap<PathBuf, Option<IndexFile>>,
    ) -> Result<Option<PathBuf>> {
        let md_files = self
            .fs
            .read_dir(dir)
            .await
            .map(|entries| {
                entries
                    .into_iter()
                    .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
                    .map(|e| e.path().to_path_buf())
                    .collect::<Vec<_>>()
            })
            .map_err(|e| DiaryxError::FileRead {
                path: dir.to_path_buf(),
                source: e,
            })?;

        let mut found_index: Option<PathBuf> = None;

        for file in md_files {
            if let Ok(index) = self.parse_index_cached(&file, cache).await
                && index.frontmatter.is_index()
            {
                if index.frontmatter.is_root() {
                    return Ok(Some(file));
                }
                if found_index.is_none() {
                    found_index = Some(file);
                }
            }
        }

        Ok(found_index)
    }

    /// Find the nearest index file by walking up directories from the given path.
    ///
    /// Starting from the parent directory of `path`, searches each directory
    /// for an index file (via `find_any_index_in_dir`), walking up the tree
    /// until one is found or the root is reached.
    pub async fn find_nearest_index(&self, path: &Path) -> Result<Option<PathBuf>> {
        let mut current = path.parent();
        while let Some(dir) = current {
            if let Some(index) = self.find_any_index_in_dir(dir).await? {
                // Don't return the file itself as its own "nearest index"
                if index != path {
                    return Ok(Some(index));
                }
            }
            current = dir.parent();
        }
        Ok(None)
    }

    /// Collect all files reachable from an index via `contents` traversal
    /// Returns a list of all files including the index itself and all nested contents
    pub async fn collect_workspace_files(&self, index_path: &Path) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        let mut visited = HashSet::new();

        // Get link format from workspace config for proper path resolution
        let link_format = self
            .get_workspace_config(index_path)
            .await
            .map(|c| c.link_format)
            .ok();

        // Get the workspace root directory (parent of root index file)
        let workspace_root = index_path.parent().unwrap_or(Path::new(".")).to_path_buf();

        self.collect_workspace_files_recursive(
            index_path,
            &mut files,
            &mut visited,
            link_format,
            &workspace_root,
        )
        .await?;
        files.sort();
        Ok(files)
    }

    /// Given the absolute path of an attachment note, reads its `attachment`
    /// frontmatter field and resolves it to the canonical binary path.
    ///
    /// Returns `None` if the file cannot be parsed or has no `attachment` field.
    pub async fn resolve_attachment_binary(
        &self,
        note_path: &Path,
        workspace_root: &Path,
        link_format: Option<LinkFormat>,
    ) -> Option<String> {
        let index = self
            .parse_index_with_hint(note_path, link_format)
            .await
            .ok()?;
        let attachment_link = index.frontmatter.attachment.as_ref()?;
        let binary_resolved = index.resolve_path(attachment_link);
        let binary_full = resolve_workspace_path(workspace_root, &binary_resolved);
        Some(canonical_workspace_path(&binary_full, workspace_root))
    }

    /// Collect the canonical workspace file set for sync/export operations.
    ///
    /// This includes all markdown files reachable from the logical workspace
    /// tree plus any attachment files declared in reachable entries'
    /// frontmatter.
    pub async fn collect_workspace_file_set(&self, index_path: &Path) -> Result<Vec<String>> {
        let workspace_root = index_path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let link_format = self
            .get_workspace_config(index_path)
            .await
            .map(|c| c.link_format)
            .ok();

        let mut files = BTreeSet::new();
        let mut visited = HashSet::new();
        self.collect_workspace_file_set_recursive(
            index_path,
            &workspace_root,
            link_format,
            &mut files,
            &mut visited,
        )
        .await?;
        Ok(files.into_iter().collect())
    }

    /// Cached variant of `collect_exclude_patterns` for use during tree builds.
    async fn collect_exclude_patterns_cached(
        &self,
        index_path: &Path,
        cache: &mut HashMap<PathBuf, Option<IndexFile>>,
    ) -> Vec<String> {
        let mut patterns = Vec::new();
        let mut current_path = index_path.to_path_buf();
        let link_format = self
            .get_workspace_config(index_path)
            .await
            .ok()
            .map(|c| c.link_format);

        loop {
            // Use parse_index_cached but apply the link_format hint afterward
            let Ok(mut index) = self.parse_index_cached(&current_path, cache).await else {
                break;
            };
            index.link_format_hint = link_format;

            patterns.extend(index.frontmatter.exclude_list().iter().cloned());

            if let Some(part_of_ref) = index.frontmatter.part_of.as_ref() {
                let parent_path = resolve_workspace_path(
                    current_path.parent().unwrap_or(Path::new(".")),
                    &index.resolve_path(part_of_ref),
                );

                if self.fs.try_exists(&parent_path).await.unwrap_or(false) {
                    current_path = parent_path;
                    continue;
                }
            }

            break;
        }

        patterns
    }

    /// Cached variant of `exclude_patterns_for_dir` for use during tree builds.
    async fn exclude_patterns_for_dir_cached(
        &self,
        dir: &Path,
        workspace_root: &Path,
        cache: &mut HashMap<PathBuf, Option<IndexFile>>,
    ) -> Vec<String> {
        let mut current = Some(dir);
        while let Some(candidate) = current {
            if !candidate.starts_with(workspace_root) {
                break;
            }

            if let Ok(Some(index)) = self.find_any_index_in_dir_cached(candidate, cache).await {
                return self.collect_exclude_patterns_cached(&index, cache).await;
            }

            current = candidate.parent();
        }

        Vec::new()
    }

    fn workspace_relative_path(&self, workspace_root: &Path, path: &Path) -> String {
        let relative = path.strip_prefix(workspace_root).unwrap_or(path);
        normalize_sync_path(&relative.to_string_lossy().replace('\\', "/"))
    }

    fn path_matches_exclude(
        &self,
        pattern: &str,
        workspace_root: &Path,
        path: &Path,
        file_name: &str,
    ) -> bool {
        let relative_path = self.workspace_relative_path(workspace_root, path);
        matches_glob_pattern(pattern, file_name) || matches_glob_pattern(pattern, &relative_path)
    }

    async fn collect_workspace_file_set_recursive(
        &self,
        path: &Path,
        workspace_root: &Path,
        link_format: Option<LinkFormat>,
        files: &mut BTreeSet<String>,
        visited: &mut HashSet<String>,
    ) -> Result<()> {
        let canonical_path = canonical_workspace_path(path, workspace_root);
        if !visited.insert(canonical_path.clone()) {
            return Ok(());
        }
        files.insert(canonical_path);

        let index = match self.parse_index_with_hint(path, link_format).await {
            Ok(index) => index,
            Err(_) => return Ok(()),
        };

        for attachment_ref in index.frontmatter.attachments_list() {
            let attachment_full_path =
                resolve_workspace_path(workspace_root, &index.resolve_path(attachment_ref));

            if self
                .fs
                .try_exists(&attachment_full_path)
                .await
                .unwrap_or(false)
            {
                files.insert(canonical_workspace_path(
                    &attachment_full_path,
                    workspace_root,
                ));

                // Follow the note's `attachment` field to include the actual binary
                if let Some(binary_canonical) = self
                    .resolve_attachment_binary(&attachment_full_path, workspace_root, link_format)
                    .await
                {
                    let binary_full =
                        resolve_workspace_path(workspace_root, Path::new(&binary_canonical));
                    if self.fs.try_exists(&binary_full).await.unwrap_or(false) {
                        files.insert(binary_canonical);
                    }
                }
            }
        }

        if !index.frontmatter.is_index() {
            return Ok(());
        }

        for child_ref in index.frontmatter.contents_list() {
            let child_full_path =
                resolve_workspace_path(workspace_root, &index.resolve_path(child_ref));

            if self.fs.try_exists(&child_full_path).await.unwrap_or(false) {
                Box::pin(self.collect_workspace_file_set_recursive(
                    &child_full_path,
                    workspace_root,
                    link_format,
                    files,
                    visited,
                ))
                .await?;
            }
        }

        Ok(())
    }

    /// Recursive helper for collecting workspace files
    async fn collect_workspace_files_recursive(
        &self,
        path: &Path,
        files: &mut Vec<PathBuf>,
        visited: &mut HashSet<PathBuf>,
        link_format: Option<LinkFormat>,
        workspace_root: &Path,
    ) -> Result<()> {
        // Canonicalize to handle relative paths consistently
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        // Avoid cycles
        if visited.contains(&canonical) {
            return Ok(());
        }
        visited.insert(canonical.clone());

        // Add this file to the list
        files.push(path.to_path_buf());

        // If this is an index file, recurse into its contents
        if let Ok(index) = self.parse_index_with_hint(path, link_format).await
            && index.frontmatter.is_index()
        {
            for child_path_str in index.frontmatter.contents_list() {
                let child_path = index.resolve_path(child_path_str);

                // Make path absolute if needed by joining with workspace root
                let absolute_child_path = if child_path.is_absolute() {
                    child_path
                } else {
                    workspace_root.join(&child_path)
                };

                // Only include if the file exists
                if self
                    .fs
                    .try_exists(&absolute_child_path)
                    .await
                    .unwrap_or(false)
                {
                    Box::pin(self.collect_workspace_files_recursive(
                        &absolute_child_path,
                        files,
                        visited,
                        link_format,
                        workspace_root,
                    ))
                    .await?;
                }
            }
        }

        Ok(())
    }

    /// Detect the workspace root from the current directory
    /// Searches current directory for a root index file
    pub async fn detect_workspace(&self, start_dir: &Path) -> Result<Option<PathBuf>> {
        // Look for root index in start directory
        if let Some(root) = self.find_root_index_in_dir(start_dir).await? {
            return Ok(Some(root));
        }

        Ok(None)
    }

    /// Combine two index files by moving contents from source to target and deleting source.
    /// Also appends the body of source to target.
    pub async fn combine_indices(&self, source_path: &Path, target_path: &Path) -> Result<()> {
        // 1. Parse both indices
        let source_index = self.parse_index(source_path).await?;
        let target_index = self.parse_index(target_path).await?;

        // Ensure both are valid indices (though parse_index should error if not valid markdown/frontmatter)
        if !source_index.frontmatter.is_index() {
            return Err(DiaryxError::YamlParse {
                path: source_path.to_path_buf(),
                message: "Source is not an index file (missing contents)".to_string(),
            });
        }
        if !target_index.frontmatter.is_index() {
            return Err(DiaryxError::YamlParse {
                path: target_path.to_path_buf(),
                message: "Target is not an index file (missing contents)".to_string(),
            });
        }

        // 2. Prepare new contents for target
        let mut new_target_contents = target_index
            .frontmatter
            .contents
            .clone()
            .unwrap_or_default();

        // Get workspace root for path formatting
        // (Use configured root if available, else derive from target path)
        let workspace_root = self
            .root_path
            .clone()
            .unwrap_or_else(|| target_path.parent().unwrap_or(Path::new(".")).to_path_buf());

        // Get target's canonical path to format links relative to it
        let target_canonical = self.get_canonical_path(target_path);

        // 3. Process source children
        if let Some(ref source_contents) = source_index.frontmatter.contents {
            for child_ref in source_contents {
                // Resolve child to absolute path
                let child_path = source_index.resolve_path(child_ref);

                // Construct absolute path (resolve_path returns workspace-relative for root paths,
                // or file-relative otherwise. It's inconsistent in return type slightly - see resolve_path docs.
                // Actually resolve_path returns PathBuf. Let's look at resolve_path implementation.
                // It returns a PathBuf. If it's workspace-relative (from /), it's returned as such (relative).
                // If it's relative, it's joined with directory.
                // Wait, resolve_path implementation says:
                // "Returns an absolute path resolved against this index file's location."
                // BUT implementation of WorkspaceRoot case returns `PathBuf::from(&parsed.path)` which is NOT absolute
                // if it's just the string from the link without workspace root.
                // Let's re-read resolve_path carefully.
                // "Returns an absolute path resolved against this index file's location." -> implementation seems to try to do that.
                // BUT for WorkspaceRoot it says "Return as PathBuf directly - callers operate relative to workspace root."
                // So result might be "Folder/file.md" (relative to workspace root).

                let mut absolute_child_path = if child_path.is_absolute() {
                    child_path
                } else {
                    // It's relative to workspace root
                    workspace_root.join(&child_path)
                };

                // Verification: Check if the file exists.
                // If not, it might be because the path resolution logic misidentified a workspace-root path
                // as a relative path (e.g. "Daily/2026/01.md" inside "Daily/2026/index.md" resolves to
                // "Daily/2026/Daily/2026/01.md").
                if !self
                    .fs
                    .try_exists(&absolute_child_path)
                    .await
                    .unwrap_or(false)
                {
                    let fallback_path = workspace_root.join(child_ref);
                    if self.fs.try_exists(&fallback_path).await.unwrap_or(false) {
                        log::info!(
                            "Resolved '{}' using fallback workspace-root strategy to {:?}",
                            child_ref,
                            fallback_path
                        );
                        absolute_child_path = fallback_path;
                    } else {
                        // If it still doesn't exist, we will likely error on write, but let's proceed
                        // to attempt write so the error is consistent (or maybe just warn?)
                        // For now, proceed, but let's log.
                        log::warn!(
                            "Child path '{}' resolved to {:?} which does not exist",
                            child_ref,
                            absolute_child_path
                        );
                    }
                }

                // Now we need to update the child's part_of to point to the target
                // We need to format the link from child to target
                let child_canonical = self.get_canonical_path(&absolute_child_path);

                // Update child's part_of
                // Format link FROM child TO target
                let part_of_link = self.format_link(&target_canonical, &child_canonical).await;

                self.set_frontmatter_property(
                    &absolute_child_path,
                    "part_of",
                    yaml::Value::String(part_of_link),
                )
                .await?;

                // Add child to target contents
                // Format link FROM target TO child
                let child_link = self.format_link(&child_canonical, &target_canonical).await;
                new_target_contents.push(child_link);
            }
        }

        // 4. Update the target index: set the new `contents` list and append the
        // body, both in place so the target index's comments and formatting
        // survive (only the `contents` value and the body change).
        let new_body = if source_index.body.trim().is_empty() {
            target_index.body
        } else if target_index.body.trim().is_empty() {
            source_index.body
        } else {
            format!("{}\n\n{}", target_index.body.trim_end(), source_index.body)
        };

        let target_raw =
            self.fs
                .read_to_string(target_path)
                .await
                .map_err(|e| DiaryxError::FileRead {
                    path: target_path.to_path_buf(),
                    source: e,
                })?;
        let contents_value = yaml::Value::Sequence(
            new_target_contents
                .into_iter()
                .map(yaml::Value::String)
                .collect(),
        );
        let with_contents =
            crate::frontmatter::set_property_in_text(&target_raw, "contents", &contents_value)?;
        let content = crate::frontmatter::replace_body(&with_contents, &new_body);

        self.fs
            .write(target_path, content.as_bytes())
            .await
            .map_err(|e| DiaryxError::FileWrite {
                path: target_path.to_path_buf(),
                source: e,
            })?;

        // 5. Delete source index file
        self.fs
            .remove_file(source_path)
            .await
            .map_err(|e| DiaryxError::FileWrite {
                path: source_path.to_path_buf(),
                source: e,
            })?;

        Ok(())
    }

    // ==================== Workspace Config Methods ====================

    /// Get the workspace configuration from the root index file's frontmatter.
    ///
    /// Reads `link_format` and other workspace-level settings from the root index.
    /// Returns default values if the properties aren't present.
    /// Known workspace config field names, used for migration from top-level
    /// frontmatter to the nested `workspace_config` section.
    const WORKSPACE_CONFIG_FIELDS: &'static [&'static str] = &[
        "link_format",
        "default_template",
        "sync_title_to_heading",
        "auto_update_timestamp",
        "auto_rename_to_title",
        "filename_style",
        "default_audience",
        "public_audience",
        "daily_entry_folder",
        "show_unlinked_files",
        "show_hidden_files",
        "theme_mode",
        "theme_preset",
        "theme_accent_hue",
        "audience_colors",
        "disabled_plugins",
        "plugins",
        "publish",
        // Legacy top-level keys, swept inline → settings file, then relocated
        // under `publish` by `migrate_publish_config`.
        "audiences",
        "audiences_migrated",
    ];

    /// Resolve the `workspace_config` value from the root index.
    ///
    /// Returns the config source (a HashMap of field→Value) and, if the config
    /// lives in an external file, the resolved path to that file.
    ///
    /// The config source is determined by:
    /// 1. If `workspace_config` is a string → parse as a link, read that file's
    ///    frontmatter extra as the config source.
    /// 2. If `workspace_config` is a mapping → use it inline (nested section).
    /// 3. Otherwise → use the root index's own extra (flat/legacy format).
    pub(crate) async fn resolve_config_source(
        &self,
        root_index_path: &Path,
    ) -> Result<(
        std::collections::HashMap<String, yaml::Value>,
        Option<PathBuf>,
    )> {
        let index = self.parse_index(root_index_path).await?;
        let extra = &index.frontmatter.extra;

        match extra.get("workspace_config") {
            // File link: workspace_config points to an external file
            Some(yaml::Value::String(link_str)) => {
                let config_path = self.resolve_root_index_link(&index, link_str);
                let config_index = self.parse_index(&config_path).await?;
                Ok((config_index.frontmatter.extra, Some(config_path)))
            }
            // Inline nested section: workspace_config is a YAML mapping
            Some(yaml::Value::Mapping(map)) => {
                // yaml::Mapping already uses String keys, convert to HashMap
                let config: std::collections::HashMap<String, yaml::Value> =
                    map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                Ok((config, None))
            }
            // No workspace_config key, or unexpected type → legacy flat format
            _ => Ok((extra.clone(), None)),
        }
    }

    // ==================== Frontmatter Helper Methods ====================
    // These are internal helpers for manipulating frontmatter in workspace operations

    /// Get a frontmatter property from a file
    async fn get_frontmatter_property(
        &self,
        path: &Path,
        key: &str,
    ) -> Result<Option<yaml::Value>> {
        let content = match self.fs.read_to_string(path).await {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(DiaryxError::FileRead {
                    path: path.to_path_buf(),
                    source: e,
                });
            }
        };

        match crate::frontmatter::parse(&content) {
            Ok(parsed) => Ok(parsed.frontmatter.get(key).cloned()),
            Err(crate::frontmatter::FrontmatterError::NoFrontmatter) => Ok(None),
            Err(crate::frontmatter::FrontmatterError::Yaml(err)) => Err(DiaryxError::Yaml(err)),
        }
    }

    /// Set a frontmatter property in a file (comment-preserving in-place edit;
    /// creates the file/frontmatter when missing).
    pub async fn set_frontmatter_property(
        &self,
        path: &Path,
        key: &str,
        value: yaml::Value,
    ) -> Result<()> {
        let content = match self.fs.read_to_string(path).await {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => {
                return Err(DiaryxError::FileRead {
                    path: path.to_path_buf(),
                    source: e,
                });
            }
        };
        let updated = crate::frontmatter::set_property_in_text(&content, key, &value)?;
        self.fs
            .write(path, updated.as_bytes())
            .await
            .map_err(|e| DiaryxError::FileWrite {
                path: path.to_path_buf(),
                source: e,
            })
    }

    /// Remove a frontmatter property from a file (comment-preserving).
    async fn remove_frontmatter_property(&self, path: &Path, key: &str) -> Result<()> {
        let content = match self.fs.read_to_string(path).await {
            Ok(c) => c,
            Err(_) => return Ok(()), // File doesn't exist, nothing to remove
        };
        let updated = crate::frontmatter::remove_property_in_text(&content, key)?;
        if updated != content {
            self.fs
                .write(path, updated.as_bytes())
                .await
                .map_err(|e| DiaryxError::FileWrite {
                    path: path.to_path_buf(),
                    source: e,
                })?;
        }
        Ok(())
    }

    /// Normalize a path string by stripping leading "./" prefix
    fn normalize_contents_path(path: &str) -> &str {
        path.strip_prefix("./").unwrap_or(path)
    }

    /// Add an entry to an index's contents list (using raw entry string).
    /// For formatted links, use `add_to_index_contents_canonical` instead.
    pub async fn add_to_index_contents(&self, index_path: &Path, entry: &str) -> Result<bool> {
        // Normalize the entry path (strip leading ./)
        let normalized_entry = Self::normalize_contents_path(entry);

        // Parse the new entry to get its canonical path for comparison
        let parsed_entry = link_parser::parse_link(normalized_entry);
        let index_canonical = self.get_canonical_path(index_path);
        let entry_canonical = link_parser::to_canonical(&parsed_entry, Path::new(&index_canonical));

        match self.get_frontmatter_property(index_path, "contents").await {
            Ok(Some(yaml::Value::Sequence(mut items))) => {
                // Check if entry already exists (comparing canonical paths)
                let already_exists = items.iter().any(|item| {
                    if let Some(s) = item.as_str() {
                        let parsed = link_parser::parse_link(s);
                        let existing_canonical =
                            link_parser::to_canonical(&parsed, Path::new(&index_canonical));
                        existing_canonical == entry_canonical
                    } else {
                        false
                    }
                });

                if !already_exists {
                    items.push(yaml::Value::String(normalized_entry.to_string()));
                    self.set_frontmatter_property(
                        index_path,
                        "contents",
                        yaml::Value::Sequence(items),
                    )
                    .await?;
                    return Ok(true);
                }
                Ok(false)
            }
            Ok(None) => {
                // Create contents with just this entry (normalized)
                let items = vec![yaml::Value::String(normalized_entry.to_string())];
                self.set_frontmatter_property(index_path, "contents", yaml::Value::Sequence(items))
                    .await?;
                Ok(true)
            }
            _ => {
                // Contents exists but isn't a sequence, or error reading - skip
                Ok(false)
            }
        }
    }

    /// Add an entry to an index's contents list using a canonical path.
    /// This formats the link according to the configured link_format.
    pub async fn add_to_index_contents_canonical(
        &self,
        index_path: &Path,
        entry_canonical: &str,
        title: &str,
    ) -> Result<bool> {
        let index_canonical = self.get_canonical_path(index_path);

        // Format the entry based on link format
        let formatted_entry = if self.root_path.is_some() {
            self.format_link_sync(entry_canonical, title, &index_canonical)
        } else {
            // Fallback: just use the canonical path
            entry_canonical.to_string()
        };

        // Extract canonical path from existing entries for comparison
        let entry_for_comparison = entry_canonical;

        match self.get_frontmatter_property(index_path, "contents").await {
            Ok(Some(yaml::Value::Sequence(mut items))) => {
                // Check if entry already exists (comparing canonical paths)
                let already_exists = items.iter().any(|item| {
                    if let Some(s) = item.as_str() {
                        // Parse the existing item to get its canonical path
                        let parsed = link_parser::parse_link(s);
                        let existing_canonical =
                            link_parser::to_canonical(&parsed, Path::new(&index_canonical));
                        existing_canonical == entry_for_comparison
                    } else {
                        false
                    }
                });

                if !already_exists {
                    items.push(yaml::Value::String(formatted_entry));
                    self.set_frontmatter_property(
                        index_path,
                        "contents",
                        yaml::Value::Sequence(items),
                    )
                    .await?;
                    return Ok(true);
                }
                Ok(false)
            }
            Ok(None) => {
                // Create contents with just this entry
                let items = vec![yaml::Value::String(formatted_entry)];
                self.set_frontmatter_property(index_path, "contents", yaml::Value::Sequence(items))
                    .await?;
                Ok(true)
            }
            _ => {
                // Contents exists but isn't a sequence, or error reading - skip
                Ok(false)
            }
        }
    }

    /// Remove an entry from an index's contents list.
    ///
    /// The `entry` can be:
    /// - A plain filename: `new-entry.md`
    /// - A relative path: `subdir/file.md`
    /// - A markdown link: `[Title](/path/to/file.md)`
    ///
    /// This properly handles markdown links in the contents list by comparing
    /// canonical paths.
    async fn remove_from_index_contents(&self, index_path: &Path, entry: &str) -> Result<bool> {
        // Parse the entry to remove to get its canonical form
        let parsed_entry = link_parser::parse_link(entry);
        let index_canonical = self.get_canonical_path(index_path);
        let entry_canonical = link_parser::to_canonical(&parsed_entry, Path::new(&index_canonical));

        self.remove_from_index_contents_impl(index_path, &index_canonical, &entry_canonical)
            .await
    }

    /// Remove an entry from an index's contents list using a pre-computed
    /// canonical path.
    ///
    /// Use this instead of [`remove_from_index_contents`] when the entry path
    /// is already a workspace-relative canonical path (e.g. from
    /// [`get_canonical_path`]). Passing a canonical path through the regular
    /// method would double-resolve it when the file lives in a subdirectory.
    async fn remove_from_index_contents_canonical(
        &self,
        index_path: &Path,
        entry_canonical: &str,
    ) -> Result<bool> {
        let index_canonical = self.get_canonical_path(index_path);
        self.remove_from_index_contents_impl(index_path, &index_canonical, entry_canonical)
            .await
    }

    async fn remove_from_index_contents_impl(
        &self,
        index_path: &Path,
        index_canonical: &str,
        entry_canonical: &str,
    ) -> Result<bool> {
        match self.get_frontmatter_property(index_path, "contents").await {
            Ok(Some(yaml::Value::Sequence(mut items))) => {
                let before_len = items.len();
                // Remove entries that match when comparing canonical paths
                items.retain(|item| {
                    if let Some(s) = item.as_str() {
                        // Parse the existing item to get its canonical path
                        let parsed = link_parser::parse_link(s);
                        let existing_canonical =
                            link_parser::to_canonical(&parsed, Path::new(&index_canonical));
                        existing_canonical != entry_canonical
                    } else {
                        true
                    }
                });

                if items.len() != before_len {
                    self.set_frontmatter_property(
                        index_path,
                        "contents",
                        yaml::Value::Sequence(items),
                    )
                    .await?;
                    return Ok(true);
                }
                Ok(false)
            }
            Ok(None) | Ok(Some(_)) => {
                // No contents property or not a sequence - nothing to remove
                Ok(false)
            }
            Err(_) => {
                // Error reading - skip
                Ok(false)
            }
        }
    }
}

fn canonical_workspace_path(path: &Path, workspace_root: &Path) -> String {
    let relative = path.strip_prefix(workspace_root).unwrap_or(path);
    let raw = relative.to_string_lossy().replace('\\', "/");
    normalize_sync_path(&raw)
}

fn resolve_workspace_path(workspace_root: &Path, resolved_path: &Path) -> PathBuf {
    if resolved_path.is_absolute() || resolved_path.starts_with(workspace_root) {
        resolved_path.to_path_buf()
    } else {
        workspace_root.join(resolved_path)
    }
}

// ---------------------------------------------------------------------------
// Sync helpers — usable without an async runtime.
// ---------------------------------------------------------------------------

/// Find the root index file in a directory using a synchronous filesystem.
///
/// A root index is a markdown file whose frontmatter has a `contents` key but
/// no `part_of` key.  This is the sync equivalent of
/// [`Workspace::find_root_index_in_dir`].
#[allow(deprecated)]
pub fn find_root_index_in_dir_sync(
    fs: &dyn crate::fs::FileSystem,
    dir: &Path,
) -> Result<Option<PathBuf>> {
    let md_files = fs.list_md_files(dir).map_err(|e| DiaryxError::FileRead {
        path: dir.to_path_buf(),
        source: e,
    })?;

    for file in md_files {
        if is_root_index_sync(fs, &file) {
            return Ok(Some(file));
        }
    }

    Ok(None)
}

/// Check whether `path` is a root index using a synchronous filesystem.
#[allow(deprecated)]
fn is_root_index_sync(fs: &dyn crate::fs::FileSystem, path: &Path) -> bool {
    let content = match fs.read_to_string(path) {
        Ok(c) => c,
        Err(_) => return false,
    };

    let frontmatter_str = match crate::frontmatter::split(&content) {
        Some((yaml_value, _)) => yaml_value,
        None => return false,
    };
    match IndexFrontmatter::from_yaml_str(frontmatter_str) {
        Ok(fm) => fm.is_root(),
        Err(_) => false,
    }
}
