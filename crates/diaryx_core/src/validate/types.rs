//! Core validation enums and their basic impls.
//!
//! This module holds the serializable `ValidationError`, `ValidationWarning`,
//! and `ValidationResult` types, plus their `description` / `can_auto_fix` /
//! `file_path` / `is_viewable` / `supports_parent_picker` metadata impls.
//! Anything that formats human-readable detail lives in
//! [`super::detail`]; the richer `*WithMeta` wrappers live in [`super::meta`].

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A validation error indicating a broken reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
#[serde(tag = "type")]
pub enum ValidationError {
    /// A file's `part_of` points to a non-existent file.
    BrokenPartOf {
        /// The file containing the broken reference
        file: PathBuf,
        /// The target path that doesn't exist
        target: String,
    },
    /// An index's `contents` references a non-existent file.
    BrokenContentsRef {
        /// The index file containing the broken reference
        index: PathBuf,
        /// The target path that doesn't exist
        target: String,
    },
    /// A file's `attachments` references a non-existent file.
    BrokenAttachment {
        /// The file containing the broken reference
        file: PathBuf,
        /// The attachment path that doesn't exist
        attachment: String,
    },
    /// A file's `links` references a non-existent file.
    BrokenLinkRef {
        /// The file containing the broken reference
        file: PathBuf,
        /// The link target that doesn't exist
        target: String,
    },
}

/// A validation warning indicating a potential issue.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
#[serde(tag = "type")]
pub enum ValidationWarning {
    /// A file exists but is not referenced by any index's contents.
    OrphanFile {
        /// The orphan file path
        file: PathBuf,
        /// Suggested index to add this to (nearest parent index in hierarchy)
        suggested_index: Option<PathBuf>,
    },
    /// A file or directory exists but is not in the contents hierarchy.
    /// Used for "List All Files" mode to show all filesystem entries.
    UnlinkedEntry {
        /// The entry path
        path: PathBuf,
        /// Whether this is a directory
        is_dir: bool,
        /// Suggested index to add this to (nearest parent index in hierarchy)
        /// For directories, this points to the index file inside the directory if one exists
        suggested_index: Option<PathBuf>,
        /// For directories with an index file, this is the path to that index file
        /// (which should be added to contents instead of the directory path)
        index_file: Option<PathBuf>,
    },
    /// Circular reference detected in workspace hierarchy.
    CircularReference {
        /// The files involved in the cycle
        files: Vec<PathBuf>,
        /// Suggested file to edit to break the cycle (the one that would break it most cleanly)
        suggested_file: Option<PathBuf>,
        /// The part_of value to remove from the suggested file
        suggested_remove_part_of: Option<String>,
    },
    /// A path in frontmatter is not portable (absolute, contains `.`, etc.)
    NonPortablePath {
        /// The file containing the non-portable path
        file: PathBuf,
        /// The property containing the path ("part_of" or "contents")
        property: String,
        /// The problematic path value
        value: String,
        /// The suggested normalized path
        suggested: String,
    },
    /// Multiple index files found in the same directory.
    MultipleIndexes {
        /// The directory containing multiple indexes
        directory: PathBuf,
        /// The index files found
        indexes: Vec<PathBuf>,
    },
    /// A binary file exists but is not referenced by any file's attachments.
    OrphanBinaryFile {
        /// The orphan binary file path
        file: PathBuf,
        /// Suggested index to add this to (if exactly one index in same directory)
        suggested_index: Option<PathBuf>,
    },
    /// A file has no `part_of` property and is not the root index (orphan/disconnected).
    MissingPartOf {
        /// The file missing the part_of property
        file: PathBuf,
        /// Suggested index to connect to (if exactly one index in same directory)
        suggested_index: Option<PathBuf>,
    },
    /// A non-markdown file is referenced in `contents`.
    ///
    /// `contents` entries must be markdown files. Binary assets belong in
    /// `attachments` wrapped by a markdown attachment note (a file with an
    /// `attachment:` property pointing at the binary).
    InvalidContentsRef {
        /// The index file containing the invalid reference
        index: PathBuf,
        /// The non-markdown file that was referenced
        target: String,
    },
    /// An `attachments` entry doesn't point at a markdown attachment note.
    ///
    /// Under the current attachment model, `attachments` must contain markdown
    /// "attachment notes" whose frontmatter carries an `attachment:` property
    /// pointing at the actual binary asset. Two shapes are rejected:
    /// - A raw binary path (`foo.HEIC`, `image.png`, …) — legacy flat format.
    /// - A markdown file that lacks an `attachment:` frontmatter property —
    ///   it's just a regular note, not an attachment note.
    InvalidAttachmentRef {
        /// The index file containing the invalid reference
        file: PathBuf,
        /// The entry as written in the `attachments` list
        target: String,
        /// Short human-readable reason (shown in the UI).
        reason: String,
        /// Structured classification of the problem, used by the autofixer.
        kind: InvalidAttachmentRefKind,
    },
    /// A file's declared `link` does not resolve back to itself.
    InvalidSelfLink {
        /// The file containing the invalid self-link
        file: PathBuf,
        /// The problematic link value
        value: String,
        /// The suggested canonical self-link
        suggested: String,
    },
    /// A file declares an outbound link but the target is missing a backlink.
    MissingBacklink {
        /// The target file that should contain the backlink
        file: PathBuf,
        /// The source file that should appear in `link_of`
        source: String,
        /// Suggested backlink value to add
        suggested: String,
    },
    /// A file has a backlink whose source file is missing or no longer links back.
    StaleBacklink {
        /// The file containing the stale backlink
        file: PathBuf,
        /// The stale backlink value in `link_of`
        value: String,
    },
    /// An index lists an attachment note whose `attachment_of` does not
    /// contain the listing index — the attachment-note backlink is missing.
    MissingAttachmentBacklink {
        /// The attachment note that should contain the backlink
        file: PathBuf,
        /// The index file that should appear in `attachment_of`
        source: String,
        /// Suggested backlink value to add
        suggested: String,
    },
    /// An attachment note has an `attachment_of` entry whose source index is
    /// missing or no longer lists the note in its `attachments`.
    StaleAttachmentBacklink {
        /// The attachment note containing the stale backlink
        file: PathBuf,
        /// The stale backlink value in `attachment_of`
        value: String,
    },
    /// The same entry appears more than once in a frontmatter list.
    ///
    /// Applies to link-bearing lists (`contents`, `attachments`, `links`,
    /// `link_of`, `attachment_of`). Duplicates are detected by canonical-link
    /// equivalence, so `[Foo](./foo.md)` and `foo.md` collapse together.
    DuplicateListEntry {
        /// The file containing the list
        file: PathBuf,
        /// The frontmatter property name (e.g. `attachments`)
        property: String,
        /// The first occurrence of the duplicated value, as it appears in YAML
        value: String,
        /// Total number of occurrences (>= 2)
        count: usize,
    },
    /// A filename contains characters that are not portable across platforms.
    /// Chrome's File System Access API rejects these even on macOS/Linux.
    NonPortableFilename {
        /// The file with the non-portable filename
        file: PathBuf,
        /// Description of the problematic character(s)
        reason: String,
        /// Suggested sanitized filename
        suggested_filename: String,
    },
}

/// Structured classification of why an `attachments` entry is rejected.
///
/// Only [`InvalidAttachmentRefKind::LegacyBinary`] is auto-fixable: it carries
/// the absolute path of the binary so the fixer can wrap it in a markdown
/// attachment note and replace the stale entry in the source index.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
#[serde(tag = "type")]
pub enum InvalidAttachmentRefKind {
    /// The entry points directly at a binary asset (legacy flat format).
    /// Auto-fixable: wrap the binary in a markdown attachment note.
    LegacyBinary {
        /// Absolute path to the binary asset the entry resolves to.
        binary_path: PathBuf,
    },
    /// The entry points at a `.md` file that lacks an `attachment:` property.
    /// Not auto-fixable — intent is ambiguous.
    NotAttachmentNote,
    /// The entry points at a file that could not be parsed.
    UnparseableNote,
}

impl ValidationWarning {
    /// Get a human-readable description of this warning.
    pub fn description(&self) -> &'static str {
        match self {
            Self::OrphanFile { .. } => "Not in any index contents",
            Self::UnlinkedEntry { is_dir: true, .. } => "Unlinked directory",
            Self::UnlinkedEntry { is_dir: false, .. } => "Unlinked file",
            Self::CircularReference { .. } => "Circular reference detected",
            Self::NonPortablePath { .. } => "Non-portable path",
            Self::MultipleIndexes { .. } => "Multiple indexes in directory",
            Self::OrphanBinaryFile { .. } => "Binary file not attached",
            Self::MissingPartOf { .. } => "Missing part_of reference",
            Self::InvalidContentsRef { .. } => "Non-markdown file in contents",
            Self::InvalidAttachmentRef { .. } => "Attachment entry is not an attachment note",
            Self::DuplicateListEntry { .. } => "Duplicate entry in frontmatter list",
            Self::InvalidSelfLink { .. } => "Invalid canonical link",
            Self::MissingBacklink { .. } => "Missing backlink",
            Self::StaleBacklink { .. } => "Stale backlink",
            Self::MissingAttachmentBacklink { .. } => "Missing attachment backlink",
            Self::StaleAttachmentBacklink { .. } => "Stale attachment backlink",
            Self::NonPortableFilename { .. } => "Non-portable filename",
        }
    }

    /// Check if this warning can be automatically fixed.
    pub fn can_auto_fix(&self) -> bool {
        match self {
            Self::OrphanFile {
                suggested_index, ..
            } => suggested_index.is_some(),
            Self::OrphanBinaryFile {
                suggested_index, ..
            } => suggested_index.is_some(),
            Self::MissingPartOf {
                suggested_index, ..
            } => suggested_index.is_some(),
            Self::UnlinkedEntry {
                suggested_index,
                is_dir,
                index_file,
                ..
            } => {
                if suggested_index.is_none() {
                    return false;
                }
                // Directories need an index file inside to be linkable
                if *is_dir { index_file.is_some() } else { true }
            }
            Self::NonPortablePath { .. } => true,
            Self::CircularReference {
                suggested_file,
                suggested_remove_part_of,
                ..
            } => suggested_file.is_some() && suggested_remove_part_of.is_some(),
            Self::MultipleIndexes { .. } => false,
            Self::InvalidContentsRef { .. } => false,
            Self::InvalidAttachmentRef { kind, .. } => {
                matches!(kind, InvalidAttachmentRefKind::LegacyBinary { .. })
            }
            Self::DuplicateListEntry { .. } => true,
            Self::InvalidSelfLink { .. } => true,
            Self::MissingBacklink { .. } => true,
            Self::StaleBacklink { .. } => true,
            Self::MissingAttachmentBacklink { .. } => true,
            Self::StaleAttachmentBacklink { .. } => true,
            Self::NonPortableFilename { .. } => true,
        }
    }

    /// Get the primary file path associated with this warning.
    pub fn file_path(&self) -> Option<&Path> {
        match self {
            Self::OrphanFile { file, .. } => Some(file),
            Self::OrphanBinaryFile { file, .. } => Some(file),
            Self::MissingPartOf { file, .. } => Some(file),
            Self::UnlinkedEntry { path, .. } => Some(path),
            Self::CircularReference { files, .. } => files.first().map(|p| p.as_path()),
            Self::NonPortablePath { file, .. } => Some(file),
            Self::MultipleIndexes { directory, .. } => Some(directory),
            Self::InvalidContentsRef { index, .. } => Some(index),
            Self::InvalidAttachmentRef { file, .. } => Some(file),
            Self::DuplicateListEntry { file, .. } => Some(file),
            Self::InvalidSelfLink { file, .. } => Some(file),
            Self::MissingBacklink { file, .. } => Some(file),
            Self::StaleBacklink { file, .. } => Some(file),
            Self::MissingAttachmentBacklink { file, .. } => Some(file),
            Self::StaleAttachmentBacklink { file, .. } => Some(file),
            Self::NonPortableFilename { file, .. } => Some(file),
        }
    }

    /// Check if the associated file can be viewed/edited (i.e., is a markdown file).
    pub fn is_viewable(&self) -> bool {
        match self {
            Self::OrphanBinaryFile { .. } => false,
            Self::UnlinkedEntry { is_dir: true, .. } => false,
            Self::MultipleIndexes { .. } => false,
            _ => self
                .file_path()
                .and_then(|p| p.extension())
                .is_some_and(|ext| ext == "md"),
        }
    }

    /// Check if this warning supports choosing a different parent index.
    pub fn supports_parent_picker(&self) -> bool {
        matches!(
            self,
            Self::OrphanFile { .. }
                | Self::OrphanBinaryFile { .. }
                | Self::MissingPartOf { .. }
                | Self::UnlinkedEntry { .. }
        )
    }

    /// Whether this warning should bubble up to the nearest ancestor index
    /// when rendered in a tree view. True for orphan-style warnings
    /// (`OrphanFile`, `OrphanBinaryFile`, `MissingPartOf`, `UnlinkedEntry`)
    /// so sidebars can display them under the parent index instead of at
    /// the workspace root. Other warnings render on the file itself.
    pub fn inherits_to_parent(&self) -> bool {
        matches!(
            self,
            Self::OrphanFile { .. }
                | Self::OrphanBinaryFile { .. }
                | Self::MissingPartOf { .. }
                | Self::UnlinkedEntry { .. }
        )
    }
}

impl ValidationError {
    /// Get a human-readable description of this error.
    pub fn description(&self) -> &'static str {
        match self {
            Self::BrokenPartOf { .. } => "Broken part_of reference",
            Self::BrokenContentsRef { .. } => "Broken contents reference",
            Self::BrokenAttachment { .. } => "Broken attachment reference",
            Self::BrokenLinkRef { .. } => "Broken link reference",
        }
    }

    /// Get the primary file path associated with this error.
    pub fn file_path(&self) -> &Path {
        match self {
            Self::BrokenPartOf { file, .. } => file,
            Self::BrokenContentsRef { index, .. } => index,
            Self::BrokenAttachment { file, .. } => file,
            Self::BrokenLinkRef { file, .. } => file,
        }
    }
}

/// Result of validating a workspace.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct ValidationResult {
    /// Validation errors (broken references)
    pub errors: Vec<ValidationError>,
    /// Validation warnings (orphans, cycles)
    pub warnings: Vec<ValidationWarning>,
    /// Number of files checked
    pub files_checked: usize,
}

impl ValidationResult {
    /// Returns true if validation passed with no errors.
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }

    /// Returns true if there are any errors or warnings.
    pub fn has_issues(&self) -> bool {
        !self.errors.is_empty() || !self.warnings.is_empty()
    }

    /// Convert to a result with computed metadata fields for frontend use.
    pub fn with_metadata(self) -> super::meta::ValidationResultWithMeta {
        super::meta::ValidationResultWithMeta {
            errors: self
                .errors
                .into_iter()
                .map(super::meta::ValidationErrorWithMeta::from)
                .collect(),
            warnings: self
                .warnings
                .into_iter()
                .map(super::meta::ValidationWarningWithMeta::from)
                .collect(),
            files_checked: self.files_checked,
        }
    }
}
