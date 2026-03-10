//! Command pattern API for unified command execution.
//!
//! This module provides a unified command pattern interface that eliminates
//! redundancy across different runtime environments (WASM, Tauri, CLI).
//!
//! # Usage
//!
//! ```ignore
//! use diaryx_core::{Command, Response, Diaryx};
//!
//! let cmd = Command::GetEntry { path: "notes/hello.md".to_string() };
//! let response = diaryx.execute(cmd).await?;
//!
//! if let Response::Entry(entry) = response {
//!     println!("Title: {:?}", entry.title);
//! }
//! ```

use std::path::PathBuf;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::link_parser::LinkFormat;
use crate::search::SearchResults;
use crate::validate::{FixResult, ValidationResult, ValidationResultWithMeta};
use crate::workspace::{TreeNode, WorkspaceConfig};

// ============================================================================
// Command Types
// ============================================================================

/// All commands that can be executed against a Diaryx instance.
///
/// Commands are serializable for cross-runtime usage (WASM, IPC, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
#[serde(tag = "type", content = "params")]
pub enum Command {
    // === Entry Operations ===
    /// Get an entry's content and metadata.
    GetEntry {
        /// Path to the entry file.
        path: String,
    },

    /// Save an entry's content.
    SaveEntry {
        /// Path to the entry file.
        path: String,
        /// New markdown content.
        content: String,
        /// Optional workspace root index path for reading workspace config.
        /// When provided, `auto_update_timestamp` from workspace config is respected.
        #[serde(default)]
        root_index_path: Option<String>,
        /// When true, detect the first-line H1 heading and sync it to the
        /// frontmatter title and filename. Used for manual save / editor blur
        /// (not auto-save) to avoid mid-typing renames.
        #[serde(default)]
        detect_h1_title: bool,
    },

    /// Create a new entry.
    CreateEntry {
        /// Path where the entry should be created.
        path: String,
        /// Optional creation options.
        #[serde(default)]
        options: CreateEntryOptions,
    },

    /// Delete an entry.
    DeleteEntry {
        /// Path to the entry to delete.
        path: String,
        /// If true, perform a hard delete (remove from filesystem).
        /// If false (default), perform a soft delete (mark as deleted in CRDT).
        #[serde(default)]
        hard_delete: bool,
    },

    /// Move/rename an entry.
    MoveEntry {
        /// Existing path to the entry file.
        from: String,
        /// New path for the entry file.
        to: String,
    },

    /// Update workspace hierarchy metadata after an external move.
    ///
    /// Unlike `MoveEntry`, this does NOT move the file on the filesystem.
    /// The file must already exist at `new_path`. Use this when an external
    /// tool (e.g., Obsidian, VS Code) has already performed the move and you
    /// need to fix up the `contents`/`part_of` frontmatter.
    SyncMoveMetadata {
        /// The file's previous path (before the move).
        old_path: String,
        /// The file's current path (after the move).
        new_path: String,
    },

    /// Update workspace hierarchy metadata after an external file creation.
    ///
    /// The file must already exist at `path`. Finds the nearest parent index
    /// and adds this file to its `contents`, then sets the file's `part_of`.
    /// Use this when an external tool (e.g., Obsidian) has created a file
    /// and you need to integrate it into the hierarchy.
    SyncCreateMetadata {
        /// Path to the newly created file.
        path: String,
    },

    /// Update workspace hierarchy metadata after an external file deletion.
    ///
    /// The file at `path` no longer exists on disk. Finds the nearest parent
    /// index and removes this file from its `contents`.
    /// Use this when an external tool (e.g., Obsidian) has deleted a file
    /// and you need to clean up the hierarchy.
    SyncDeleteMetadata {
        /// Path to the deleted file (file no longer exists on disk).
        path: String,
    },

    /// Rename an entry file.
    RenameEntry {
        /// Path to the entry to rename.
        path: String,
        /// New filename (e.g., "new-name.md").
        new_filename: String,
    },

    /// Duplicate an entry, creating a copy.
    DuplicateEntry {
        /// Path to the entry to duplicate.
        path: String,
    },

    /// Convert a leaf file to an index file with a directory.
    ConvertToIndex {
        /// Path to the leaf file to convert.
        path: String,
    },

    /// Convert an empty index file back to a leaf file.
    ConvertToLeaf {
        /// Path to the index file to convert.
        path: String,
    },

    /// Create a new child entry under a parent.
    CreateChildEntry {
        /// Path to the parent entry.
        parent_path: String,
    },

    /// Attach an existing entry to a parent index.
    AttachEntryToParent {
        /// Path to the entry to attach.
        entry_path: String,
        /// Path to the parent index file.
        parent_path: String,
    },

    // === Workspace Operations ===
    /// Find the root index file in a directory.
    /// Returns the path to the root index (a file with `contents` but no `part_of`).
    FindRootIndex {
        /// Directory to search in.
        directory: String,
    },

    /// Get all unique audience tags used in a workspace.
    GetAvailableAudiences {
        /// Path to the workspace root index file.
        path: String,
    },

    /// Get the effective audience for an entry, resolving inheritance.
    ///
    /// If the entry has an explicit `audience`, returns it directly.
    /// Otherwise walks up the `part_of` chain to find the nearest ancestor
    /// with an audience set.
    GetEffectiveAudience {
        /// Path to the entry file.
        path: String,
    },

    /// Get the workspace tree structure.
    GetWorkspaceTree {
        /// Optional path to a specific workspace.
        path: Option<String>,
        /// Optional maximum depth to traverse.
        depth: Option<u32>,
        /// Optional audience filter. When set, only entries visible to this audience are included.
        audience: Option<String>,
    },

    /// Get the filesystem tree (for "Show All Files" mode).
    GetFilesystemTree {
        /// Optional path to the workspace directory.
        path: Option<String>,
        /// Whether to include hidden files.
        #[serde(default)]
        show_hidden: bool,
        /// Optional maximum depth to traverse.
        depth: Option<u32>,
    },

    /// Create a new workspace.
    CreateWorkspace {
        /// Path where the workspace should be created.
        path: Option<String>,
        /// Name of the workspace.
        name: Option<String>,
    },

    // === Frontmatter Operations ===
    /// Get all frontmatter properties for an entry.
    GetFrontmatter {
        /// Path to the entry file.
        path: String,
    },

    /// Set a frontmatter property.
    SetFrontmatterProperty {
        /// Path to the entry file.
        path: String,
        /// Property key.
        key: String,
        /// Property value.
        value: JsonValue,
        /// Optional workspace root index path for reading workspace config.
        /// When provided, `sync_title_to_heading` is respected for title changes.
        #[serde(default)]
        root_index_path: Option<String>,
    },

    /// Remove a frontmatter property.
    RemoveFrontmatterProperty {
        /// Path to the entry file.
        path: String,
        /// Property key to remove.
        key: String,
    },

    // === Search ===
    /// Search the workspace for entries.
    SearchWorkspace {
        /// Search pattern.
        pattern: String,
        /// Search options.
        #[serde(default)]
        options: SearchOptions,
    },

    // === Validation ===
    /// Validate workspace links.
    ValidateWorkspace {
        /// Optional path to workspace.
        path: Option<String>,
    },

    /// Validate a single file's links.
    ValidateFile {
        /// Path to the file to validate.
        path: String,
    },

    /// Fix a broken part_of reference.
    FixBrokenPartOf {
        /// Path to the file with the broken reference.
        path: String,
    },

    /// Fix a broken contents reference.
    FixBrokenContentsRef {
        /// Path to the index file.
        index_path: String,
        /// The broken reference to remove.
        target: String,
    },

    /// Fix a broken attachment reference.
    FixBrokenAttachment {
        /// Path to the file with the broken attachment.
        path: String,
        /// The broken attachment reference.
        attachment: String,
    },

    /// Fix a non-portable path.
    FixNonPortablePath {
        /// Path to the file.
        path: String,
        /// Property name.
        property: String,
        /// Current value.
        old_value: String,
        /// New value.
        new_value: String,
    },

    /// Add an unlisted file to an index's contents.
    FixUnlistedFile {
        /// Path to the index file.
        index_path: String,
        /// Path to the file to add.
        file_path: String,
    },

    /// Add an orphan binary file to an index's attachments.
    FixOrphanBinaryFile {
        /// Path to the index file.
        index_path: String,
        /// Path to the binary file.
        file_path: String,
    },

    /// Fix a missing part_of reference.
    FixMissingPartOf {
        /// Path to the file missing part_of.
        file_path: String,
        /// Path to the index file to reference.
        index_path: String,
    },

    /// Fix all validation issues.
    FixAll {
        /// The validation result to fix.
        validation_result: ValidationResult,
    },

    /// Fix a circular reference by removing part_of from a file.
    FixCircularReference {
        /// Path to the file to edit.
        file_path: String,
        /// The part_of value to remove.
        part_of_value: String,
    },

    /// Get available parent indexes for a file (for "Choose parent" picker).
    GetAvailableParentIndexes {
        /// Path to the file that needs a parent.
        file_path: String,
        /// Workspace root to limit scope.
        workspace_root: String,
    },

    // === Attachments ===
    /// Get attachments for an entry.
    GetAttachments {
        /// Path to the entry file.
        path: String,
    },

    /// Register an already-written attachment in entry frontmatter.
    RegisterAttachment {
        /// Path to the entry file.
        entry_path: String,
        /// Filename for the attachment.
        filename: String,
    },

    /// Delete an attachment.
    DeleteAttachment {
        /// Path to the entry file.
        entry_path: String,
        /// Path to the attachment.
        attachment_path: String,
    },

    /// Get attachment data.
    GetAttachmentData {
        /// Path to the entry file.
        entry_path: String,
        /// Path to the attachment.
        attachment_path: String,
    },

    /// Resolve an attachment path to its storage path (for use with readBinary).
    ///
    /// Returns the resolved filesystem-relative path as a string, allowing
    /// callers to use the efficient binary transfer path (readBinary) instead
    /// of the JSON-serialized GetAttachmentData command.
    ResolveAttachmentPath {
        /// Path to the entry file.
        entry_path: String,
        /// Path to the attachment (link ref or relative path).
        attachment_path: String,
    },

    /// Move an attachment from one entry to another.
    MoveAttachment {
        /// Path to the source entry file.
        source_entry_path: String,
        /// Path to the target entry file.
        target_entry_path: String,
        /// Relative path to the attachment (e.g., "_attachments/image.png").
        attachment_path: String,
        /// Optional new filename (for handling collisions).
        new_filename: Option<String>,
    },

    /// Get attachments from current entry and all ancestor indexes.
    /// Traverses up the `part_of` chain to collect inherited attachments.
    GetAncestorAttachments {
        /// Path to the entry file.
        path: String,
    },

    // === File System ===
    /// Check if a file exists.
    FileExists {
        /// Path to check.
        path: String,
    },

    /// Read a file's content.
    ReadFile {
        /// Path to read.
        path: String,
    },

    /// Write content to a file.
    WriteFile {
        /// Path to write.
        path: String,
        /// Content to write.
        content: String,
    },

    /// Delete a file.
    DeleteFile {
        /// Path to delete.
        path: String,
    },

    /// Delete all files and subdirectories within a directory.
    ClearDirectory {
        /// Path to the directory to clear.
        path: String,
    },

    /// Write a file with metadata as YAML frontmatter + body content.
    /// This generates the YAML frontmatter from the metadata and writes it to the file.
    WriteFileWithMetadata {
        /// Path to the file to write.
        path: String,
        /// File metadata to write as frontmatter.
        metadata: serde_json::Value,
        /// Body content (markdown after frontmatter).
        body: String,
    },

    /// Update file's frontmatter metadata, preserving existing body.
    /// If body is provided, it replaces the existing body.
    UpdateFileMetadata {
        /// Path to the file to update.
        path: String,
        /// File metadata to write as frontmatter.
        metadata: serde_json::Value,
        /// Optional new body content. If not provided, existing body is preserved.
        body: Option<String>,
    },

    // === Storage ===
    /// Get storage usage information.
    GetStorageUsage,

    // ==================== Workspace Configuration Commands ====================
    /// Get the link format setting from the workspace root index.
    ///
    /// Returns the current link format (MarkdownRoot, MarkdownRelative, etc.).
    GetLinkFormat {
        /// Path to the workspace root index file.
        root_index_path: String,
    },

    /// Set the link format setting in the workspace root index.
    ///
    /// This updates the `link_format` property in the root index's frontmatter.
    SetLinkFormat {
        /// Path to the workspace root index file.
        root_index_path: String,
        /// The link format to set (one of: markdown_root, markdown_relative, plain_relative, plain_canonical).
        format: String,
    },

    /// Get the full workspace configuration from the root index.
    ///
    /// Returns WorkspaceConfig with link_format and other settings.
    GetWorkspaceConfig {
        /// Path to the workspace root index file.
        root_index_path: String,
    },

    /// Generate a filename from a title using the workspace's filename_style setting.
    ///
    /// Returns the generated filename (with .md extension) as a String.
    GenerateFilename {
        /// The entry title to convert to a filename.
        title: String,
        /// Path to the workspace root index file (to read filename_style config).
        /// If None, uses the default style (preserve).
        root_index_path: Option<String>,
    },

    /// Set a workspace configuration field in the root index file's frontmatter.
    SetWorkspaceConfig {
        /// Path to the workspace root index file.
        root_index_path: String,
        /// Field name to set (e.g., "filename_style", "default_audience").
        field: String,
        /// Value to set (stored as a string in frontmatter).
        value: String,
    },

    /// Convert all links in workspace files to a target format.
    ///
    /// This scans files and rewrites `part_of`, `contents`, and `attachments`
    /// properties.
    /// Returns the count of files modified and links converted.
    ConvertLinks {
        /// Path to the workspace root index file.
        root_index_path: String,
        /// The target link format.
        format: String,
        /// Optional specific file path to convert (if None, converts entire workspace).
        path: Option<String>,
        /// If true, only report what would be changed without modifying files.
        #[serde(default)]
        dry_run: bool,
    },

    // ==================== Link Parser Commands ====================
    /// Run link parser operations from frontend/backend callers.
    ///
    /// This exposes canonical link parsing/conversion logic so web clients
    /// don't need to duplicate path parsing semantics.
    LinkParser {
        /// The link parser operation to execute.
        operation: LinkParserOperation,
    },

    // ==================== Naming / URL Validation Commands ====================
    /// Validate and normalize a workspace name for creation.
    ///
    /// Checks that the name is non-empty and unique among existing local
    /// and (optionally) server workspace names. Returns the trimmed name.
    ValidateWorkspaceName {
        /// The workspace name to validate.
        name: String,
        /// Existing local workspace names (for uniqueness check).
        existing_local_names: Vec<String>,
        /// Existing server workspace names (optional, for sync uniqueness check).
        #[serde(default)]
        existing_server_names: Option<Vec<String>>,
    },

    /// Validate a publishing site slug.
    ///
    /// Must be 3–64 characters of lowercase letters, digits, or hyphens.
    ValidatePublishingSlug {
        /// The slug to validate.
        slug: String,
    },

    /// Normalize a server URL (trim whitespace, add `https://` if no scheme).
    NormalizeServerUrl {
        /// The URL to normalize.
        url: String,
    },

    // === Plugin Operations ===
    /// Execute a plugin-specific command.
    ///
    /// Routes to the named plugin via the [`PluginRegistry`](crate::plugin::PluginRegistry).
    /// All plugin commands (sync, publish, custom) use this variant.
    PluginCommand {
        /// Plugin identifier (e.g., `"sync"`, `"publish"`).
        plugin: String,
        /// Command name within the plugin.
        command: String,
        /// Command parameters as JSON.
        params: JsonValue,
    },

    /// Get manifests for all registered plugins.
    GetPluginManifests,

    /// Get a plugin's configuration.
    GetPluginConfig {
        /// Plugin identifier.
        plugin: String,
    },

    /// Set a plugin's configuration.
    SetPluginConfig {
        /// Plugin identifier.
        plugin: String,
        /// New configuration value.
        config: JsonValue,
    },
}

impl Command {
    /// Normalize all path fields to workspace-relative paths.
    ///
    /// This ensures commands work correctly regardless of whether paths are
    /// absolute OS paths (as sent by Tauri) or already workspace-relative
    /// (as sent by WASM). The normalizer should strip the workspace root
    /// prefix from absolute paths; it is a no-op for already-relative paths.
    pub fn normalize_paths(&mut self, normalizer: impl Fn(&str) -> String) {
        match self {
            // --- Variants with a single `path` field ---
            Command::GetEntry { path }
            | Command::DeleteEntry { path, .. }
            | Command::SyncCreateMetadata { path }
            | Command::SyncDeleteMetadata { path }
            | Command::RenameEntry { path, .. }
            | Command::DuplicateEntry { path }
            | Command::ConvertToIndex { path }
            | Command::ConvertToLeaf { path }
            | Command::GetFrontmatter { path }
            | Command::RemoveFrontmatterProperty { path, .. }
            | Command::ValidateFile { path }
            | Command::FixBrokenPartOf { path }
            | Command::FixBrokenAttachment { path, .. }
            | Command::FixNonPortablePath { path, .. }
            | Command::GetAttachments { path }
            | Command::GetAncestorAttachments { path }
            | Command::FileExists { path }
            | Command::ReadFile { path }
            | Command::WriteFile { path, .. }
            | Command::DeleteFile { path }
            | Command::ClearDirectory { path }
            | Command::WriteFileWithMetadata { path, .. }
            | Command::UpdateFileMetadata { path, .. }
            | Command::GetAvailableAudiences { path }
            | Command::GetEffectiveAudience { path } => {
                *path = normalizer(path);
            }

            // --- Variants with `path` as Option<String> (file paths, not directories) ---
            Command::GetWorkspaceTree { path, .. } | Command::ValidateWorkspace { path } => {
                if let Some(p) = path {
                    *p = normalizer(p);
                }
            }

            // --- Workspace directory paths — NOT normalized (stripping would yield "") ---
            Command::GetFilesystemTree { .. } | Command::CreateWorkspace { .. } => {}

            // --- Variants with path + optional root_index_path ---
            Command::SaveEntry {
                path,
                root_index_path,
                ..
            } => {
                *path = normalizer(path);
                if let Some(rip) = root_index_path {
                    *rip = normalizer(rip);
                }
            }

            Command::CreateEntry { path, options } => {
                *path = normalizer(path);
                if let Some(rip) = &mut options.root_index_path {
                    *rip = normalizer(rip);
                }
            }

            Command::SetFrontmatterProperty {
                path,
                root_index_path,
                ..
            } => {
                *path = normalizer(path);
                if let Some(rip) = root_index_path {
                    *rip = normalizer(rip);
                }
            }

            // --- Entry pair paths ---
            Command::MoveEntry { from, to } => {
                *from = normalizer(from);
                *to = normalizer(to);
            }

            Command::SyncMoveMetadata { old_path, new_path } => {
                *old_path = normalizer(old_path);
                *new_path = normalizer(new_path);
            }

            Command::CreateChildEntry { parent_path } => {
                *parent_path = normalizer(parent_path);
            }

            Command::AttachEntryToParent {
                entry_path,
                parent_path,
            } => {
                *entry_path = normalizer(entry_path);
                *parent_path = normalizer(parent_path);
            }

            // --- Workspace directory paths — NOT normalized ---
            Command::FindRootIndex { .. } => {}

            // --- Search ---
            Command::SearchWorkspace { options, .. } => {
                if let Some(wp) = &mut options.workspace_path {
                    *wp = normalizer(wp);
                }
            }

            // --- Fix commands ---
            Command::FixBrokenContentsRef { index_path, .. } => {
                *index_path = normalizer(index_path);
            }

            Command::FixUnlistedFile {
                index_path,
                file_path,
            }
            | Command::FixOrphanBinaryFile {
                index_path,
                file_path,
            }
            | Command::FixMissingPartOf {
                file_path,
                index_path,
            } => {
                *index_path = normalizer(index_path);
                *file_path = normalizer(file_path);
            }

            Command::FixCircularReference { file_path, .. } => {
                *file_path = normalizer(file_path);
            }

            Command::GetAvailableParentIndexes {
                file_path,
                workspace_root,
            } => {
                *file_path = normalizer(file_path);
                *workspace_root = normalizer(workspace_root);
            }

            Command::FixAll { .. } => {}

            // --- Attachments (entry_path only; attachment_path is a link ref) ---
            Command::RegisterAttachment { entry_path, .. }
            | Command::DeleteAttachment { entry_path, .. }
            | Command::GetAttachmentData { entry_path, .. }
            | Command::ResolveAttachmentPath { entry_path, .. } => {
                *entry_path = normalizer(entry_path);
            }

            Command::MoveAttachment {
                source_entry_path,
                target_entry_path,
                ..
            } => {
                *source_entry_path = normalizer(source_entry_path);
                *target_entry_path = normalizer(target_entry_path);
            }

            // --- Storage ---
            Command::GetStorageUsage => {}

            // --- Workspace configuration ---
            Command::GetLinkFormat { root_index_path }
            | Command::SetLinkFormat {
                root_index_path, ..
            }
            | Command::GetWorkspaceConfig { root_index_path }
            | Command::SetWorkspaceConfig {
                root_index_path, ..
            } => {
                *root_index_path = normalizer(root_index_path);
            }

            Command::GenerateFilename {
                root_index_path, ..
            } => {
                if let Some(rip) = root_index_path {
                    *rip = normalizer(rip);
                }
            }

            Command::ConvertLinks {
                root_index_path,
                path,
                ..
            } => {
                *root_index_path = normalizer(root_index_path);
                if let Some(p) = path {
                    *p = normalizer(p);
                }
            }

            // --- Link parser (normalize path fields inside the operation) ---
            Command::LinkParser { operation } => match operation {
                LinkParserOperation::Parse { .. } => {}
                LinkParserOperation::ToCanonical {
                    current_file_path, ..
                } => {
                    *current_file_path = normalizer(current_file_path);
                }
                LinkParserOperation::Format {
                    canonical_path,
                    from_canonical_path,
                    ..
                } => {
                    *canonical_path = normalizer(canonical_path);
                    *from_canonical_path = normalizer(from_canonical_path);
                }
                LinkParserOperation::Convert {
                    current_file_path, ..
                } => {
                    *current_file_path = normalizer(current_file_path);
                }
            },

            // --- Naming / URL Validation Commands ---
            Command::ValidateWorkspaceName { .. }
            | Command::ValidatePublishingSlug { .. }
            | Command::NormalizeServerUrl { .. }
            | Command::PluginCommand { .. }
            | Command::GetPluginManifests
            | Command::GetPluginConfig { .. }
            | Command::SetPluginConfig { .. } => {}
        }
    }
}

// ============================================================================
// Result Types
// ============================================================================

/// Result of creating a child entry, with details about any parent conversion.
///
/// When creating a child under a leaf file, the leaf is converted to an index first.
/// This struct provides both the new child path and the (possibly new) parent path,
/// allowing the frontend to correctly update the tree and navigation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct CreateChildResult {
    /// Path to the newly created child entry.
    pub child_path: String,
    /// Current path to the parent entry (may differ from input if converted to index).
    pub parent_path: String,
    /// True if the parent was converted from a leaf to an index.
    pub parent_converted: bool,
    /// Original parent path before conversion (only set if parent_converted is true).
    #[cfg_attr(feature = "typescript", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_parent_path: Option<String>,
}

// ============================================================================
// Response Types
// ============================================================================

/// Response from a command execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
#[serde(tag = "type", content = "data")]
pub enum Response {
    /// Command completed successfully with no data.
    Ok,

    /// String response.
    String(String),

    /// Boolean response.
    Bool(bool),

    /// Entry data response.
    Entry(EntryData),

    /// Tree node response.
    Tree(TreeNode),

    /// Frontmatter response.
    Frontmatter(IndexMap<String, JsonValue>),

    /// Search results response.
    SearchResults(SearchResults),

    /// Validation result response (with computed metadata for frontend).
    ValidationResult(ValidationResultWithMeta),

    /// Fix result response.
    FixResult(FixResult),

    /// Fix summary response.
    FixSummary(FixSummary),

    /// String array response.
    Strings(Vec<String>),

    /// Bytes response (base64 encoded).
    Bytes(Vec<u8>),

    /// Storage info response.
    StorageInfo(StorageInfo),

    /// Ancestor attachments response.
    AncestorAttachments(AncestorAttachmentsResult),

    /// Effective audience response.
    EffectiveAudience(EffectiveAudienceResult),

    /// Link format response.
    LinkFormat(LinkFormat),

    /// Workspace config response.
    WorkspaceConfig(WorkspaceConfig),

    /// Convert links result response.
    ConvertLinksResult(ConvertLinksResult),

    /// Create child entry result (includes parent conversion info).
    CreateChildResult(CreateChildResult),

    /// Link parser operation result.
    LinkParserResult(LinkParserResult),

    /// Result from a plugin command.
    PluginResult(JsonValue),

    /// Plugin manifests response.
    PluginManifests(Vec<crate::plugin::PluginManifest>),
}

// ============================================================================
// Helper Types
// ============================================================================

/// Data for a single diary entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct EntryData {
    /// Path to the entry.
    pub path: PathBuf,
    /// Title from frontmatter.
    pub title: Option<String>,
    /// All frontmatter properties.
    pub frontmatter: IndexMap<String, JsonValue>,
    /// Body content (after frontmatter).
    pub content: String,
}

/// Options for creating a new entry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct CreateEntryOptions {
    /// Title for the entry.
    pub title: Option<String>,
    /// Parent to attach to.
    pub part_of: Option<String>,
    /// Template to use.
    pub template: Option<String>,
    /// Optional workspace root index path for reading workspace config.
    /// When provided, `default_template` from workspace config is used as fallback.
    #[serde(default)]
    pub root_index_path: Option<String>,
}

/// Options for searching entries.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct SearchOptions {
    /// Workspace path to search in.
    pub workspace_path: Option<String>,
    /// Whether to search frontmatter.
    #[serde(default)]
    pub search_frontmatter: bool,
    /// Specific property to search.
    pub property: Option<String>,
    /// Case sensitive search.
    #[serde(default)]
    pub case_sensitive: bool,
}

/// An exported file with its path and content.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct ExportedFile {
    /// Relative path.
    pub path: String,
    /// File content.
    pub content: String,
}

/// A binary file with its path and data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct BinaryExportFile {
    /// Relative path.
    pub path: String,
    /// Binary data.
    pub data: Vec<u8>,
}

/// Binary file path info (without data) for efficient transfer.
/// Use this when you need to list files and fetch data separately.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct BinaryFileInfo {
    /// Source path (absolute, for reading).
    pub source_path: String,
    /// Relative path (for zip file structure).
    pub relative_path: String,
}

/// Information about storage usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct StorageInfo {
    /// Bytes used.
    pub used: u64,
    /// Storage limit (if any).
    pub limit: Option<u64>,
    /// Attachment size limit.
    pub attachment_limit: Option<u64>,
}

/// Summary of fix operations performed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct FixSummary {
    /// Results from fixing errors.
    pub error_fixes: Vec<FixResult>,
    /// Results from fixing warnings.
    pub warning_fixes: Vec<FixResult>,
    /// Total number of issues fixed.
    pub total_fixed: usize,
    /// Total number of fixes that failed.
    pub total_failed: usize,
}

/// A single entry's attachments in the ancestor chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct AncestorAttachmentEntry {
    /// Path to the entry file.
    pub entry_path: String,
    /// Title of the entry (from frontmatter).
    pub entry_title: Option<String>,
    /// List of attachment paths for this entry.
    pub attachments: Vec<String>,
}

/// Result of GetAncestorAttachments command.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct AncestorAttachmentsResult {
    /// Attachments from current entry and all ancestors.
    /// Ordered from current entry first, then ancestors (closest to root).
    pub entries: Vec<AncestorAttachmentEntry>,
}

/// Result of resolving effective audience for an entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct EffectiveAudienceResult {
    /// The resolved audience tags (empty if none found).
    pub tags: Vec<String>,
    /// Whether the audience was inherited from an ancestor (false if explicit).
    pub inherited: bool,
    /// Title of the ancestor entry the audience was inherited from.
    #[cfg_attr(feature = "typescript", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_title: Option<String>,
    /// Whether this entry has a parent and can potentially inherit.
    pub can_inherit: bool,
    /// Whether this entry's audience was resolved from the workspace `default_audience`
    /// config (i.e., the entry has no explicit or inherited audience tags).
    pub default_audience_applied: bool,
}

/// Result of converting links to a new format.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct ConvertLinksResult {
    /// Number of files that were modified (or would be modified in dry-run).
    pub files_modified: usize,
    /// Number of links that were converted (or would be converted in dry-run).
    pub links_converted: usize,
    /// List of file paths that were modified.
    pub modified_files: Vec<String>,
    /// Whether this was a dry run (no actual changes made).
    pub dry_run: bool,
}

/// Link parser operation selector.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
#[serde(tag = "type", content = "params", rename_all = "snake_case")]
pub enum LinkParserOperation {
    /// Parse a link string into title/path/path type.
    Parse {
        /// Link string to parse.
        link: String,
    },
    /// Resolve a link string to canonical (workspace-relative) path.
    ToCanonical {
        /// Link string to resolve.
        link: String,
        /// Canonical path of the file containing the link.
        current_file_path: String,
        /// Optional hint for resolving ambiguous links.
        #[serde(default)]
        link_format_hint: Option<LinkFormat>,
    },
    /// Format a canonical path as a link string.
    Format {
        /// Canonical target path.
        canonical_path: String,
        /// Display title.
        title: String,
        /// Output format.
        format: LinkFormat,
        /// Canonical path of the file containing the link.
        from_canonical_path: String,
    },
    /// Convert an input link string to another format.
    Convert {
        /// Original link string.
        link: String,
        /// Desired output format.
        target_format: LinkFormat,
        /// Canonical path of the file containing the link.
        current_file_path: String,
        /// Optional hint for interpreting ambiguous source links.
        #[serde(default)]
        source_format_hint: Option<LinkFormat>,
    },
}

/// Path classification from the link parser.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
#[serde(rename_all = "snake_case")]
pub enum LinkPathType {
    /// Link path starts at workspace root (`/path/file.md`).
    WorkspaceRoot,
    /// Link path is explicitly relative (`./` or `../`).
    Relative,
    /// Link path is plain/ambiguous (`path/file.md`).
    Ambiguous,
}

/// Parsed link payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct ParsedLinkResult {
    /// Markdown link title (if present).
    pub title: Option<String>,
    /// Extracted path component.
    pub path: String,
    /// Path classification.
    pub path_type: LinkPathType,
}

/// Result of running a link parser operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum LinkParserResult {
    /// Structured parse output.
    Parsed(ParsedLinkResult),
    /// String output from canonicalize/format/convert operations.
    String(String),
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = Command::GetEntry {
            path: "notes/hello.md".to_string(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("GetEntry"));
        assert!(json.contains("notes/hello.md"));

        // Deserialize back
        let cmd2: Command = serde_json::from_str(&json).unwrap();
        if let Command::GetEntry { path } = cmd2 {
            assert_eq!(path, "notes/hello.md");
        } else {
            panic!("Wrong command type");
        }
    }

    #[test]
    fn test_response_serialization() {
        let resp = Response::String("hello".to_string());
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("String"));
        assert!(json.contains("hello"));

        // Deserialize back
        let resp2: Response = serde_json::from_str(&json).unwrap();
        if let Response::String(s) = resp2 {
            assert_eq!(s, "hello");
        } else {
            panic!("Wrong response type");
        }
    }

    #[test]
    fn test_create_entry_options_default() {
        let opts = CreateEntryOptions::default();
        assert!(opts.title.is_none());
        assert!(opts.part_of.is_none());
        assert!(opts.template.is_none());
    }

    #[test]
    fn test_search_options_default() {
        let opts = SearchOptions::default();
        assert!(!opts.search_frontmatter);
        assert!(!opts.case_sensitive);
        assert!(opts.property.is_none());
    }

    #[test]
    fn test_normalize_paths_normalizes_entry_path() {
        let mut cmd = Command::GetEntry {
            path: "/workspace/notes/day.md".to_string(),
        };

        cmd.normalize_paths(|p| p.trim_start_matches("/workspace/").to_string());

        match cmd {
            Command::GetEntry { path } => {
                assert_eq!(path, "notes/day.md");
            }
            other => panic!("Expected GetEntry, got {:?}", other),
        }
    }
}
