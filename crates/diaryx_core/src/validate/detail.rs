//! Contextual one-line detail strings for validation warnings and errors.
//!
//! These strings are the "subject → target" style summaries that UIs (CLI,
//! web sidebar) render next to the short `description()` header. Keeping them
//! here keeps per-variant formatting out of `types.rs` and out of consumers.
//!
//! Convention: `description()` is the header ("Missing backlink"), and
//! `warning_detail()` / `error_detail()` produce the subject phrase that
//! follows it ("note.md should list README.md in link_of"). The intended
//! render shape is `"{description}: {detail}"`.

use std::path::Path;

use super::types::{ValidationError, ValidationWarning};

impl ValidationWarning {
    /// One-line contextual summary. Pair with [`Self::description`] as
    /// `"{description}: {detail}"` for a human-readable render.
    pub fn detail(&self) -> String {
        warning_detail(self)
    }
}

impl ValidationError {
    /// One-line contextual summary. Pair with [`Self::description`] as
    /// `"{description}: {detail}"` for a human-readable render.
    pub fn detail(&self) -> String {
        error_detail(self)
    }
}

fn display(path: &Path) -> String {
    path.display().to_string()
}

/// Contextual one-line detail for a [`ValidationWarning`].
pub fn warning_detail(warning: &ValidationWarning) -> String {
    match warning {
        ValidationWarning::OrphanFile {
            file,
            suggested_index,
        } => match suggested_index {
            Some(index) => format!(
                "{} is not in any index (suggested parent: {})",
                display(file),
                display(index)
            ),
            None => format!("{} is not in any index", display(file)),
        },
        ValidationWarning::UnlinkedEntry {
            path,
            is_dir,
            suggested_index,
            index_file,
        } => {
            let kind = if *is_dir { "directory" } else { "file" };
            match (suggested_index, index_file) {
                (Some(index), Some(inner)) => format!(
                    "{} {} is not linked (suggested parent: {}, inner index: {})",
                    kind,
                    display(path),
                    display(index),
                    display(inner)
                ),
                (Some(index), None) => format!(
                    "{} {} is not linked (suggested parent: {})",
                    kind,
                    display(path),
                    display(index)
                ),
                _ => format!("{} {} is not linked", kind, display(path)),
            }
        }
        ValidationWarning::CircularReference {
            files,
            suggested_file,
            suggested_remove_part_of,
        } => {
            let chain = files
                .iter()
                .map(|p| display(p))
                .collect::<Vec<_>>()
                .join(" → ");
            match (suggested_file, suggested_remove_part_of) {
                (Some(f), Some(ref_to_remove)) => format!(
                    "cycle {} (suggest removing '{}' from {})",
                    chain,
                    ref_to_remove,
                    display(f)
                ),
                _ => format!("cycle {}", chain),
            }
        }
        ValidationWarning::NonPortablePath {
            file,
            property,
            value,
            suggested,
        } => format!(
            "{} in {} has non-portable value '{}' (suggested: '{}')",
            property,
            display(file),
            value,
            suggested
        ),
        ValidationWarning::MultipleIndexes { directory, indexes } => {
            let names = indexes
                .iter()
                .map(|p| display(p))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "{} contains multiple indexes: {}",
                display(directory),
                names
            )
        }
        ValidationWarning::OrphanBinaryFile {
            file,
            suggested_index,
        } => match suggested_index {
            Some(index) => format!(
                "{} is not attached (suggested index: {})",
                display(file),
                display(index)
            ),
            None => format!("{} is not attached", display(file)),
        },
        ValidationWarning::MissingPartOf {
            file,
            suggested_index,
        } => match suggested_index {
            Some(index) => format!(
                "{} has no part_of (suggested parent: {})",
                display(file),
                display(index)
            ),
            None => format!("{} has no part_of", display(file)),
        },
        ValidationWarning::InvalidContentsRef { index, target } => format!(
            "{} references non-markdown '{}' in contents",
            display(index),
            target
        ),
        ValidationWarning::InvalidAttachmentRef {
            file,
            target,
            reason,
            ..
        } => format!(
            "{} has invalid attachment '{}': {}",
            display(file),
            target,
            reason
        ),
        ValidationWarning::InvalidSelfLink {
            file,
            value,
            suggested,
        } => format!(
            "{} has self-link '{}' (expected '{}')",
            display(file),
            value,
            suggested
        ),
        ValidationWarning::MissingBacklink {
            file,
            source,
            suggested: _,
        } => format!("{} should list {} in link_of", display(file), source),
        ValidationWarning::StaleBacklink { file, value } => {
            format!("{} has stale link_of entry '{}'", display(file), value)
        }
        ValidationWarning::MissingAttachmentBacklink {
            file,
            source,
            suggested: _,
        } => format!("{} should list {} in attachment_of", display(file), source),
        ValidationWarning::StaleAttachmentBacklink { file, value } => format!(
            "{} has stale attachment_of entry '{}'",
            display(file),
            value
        ),
        ValidationWarning::DuplicateListEntry {
            file,
            property,
            value,
            count,
        } => format!(
            "{} lists '{}' in {} {} times",
            display(file),
            value,
            property,
            count
        ),
        ValidationWarning::NonPortableFilename {
            file,
            reason,
            suggested_filename,
        } => format!(
            "{}: {} (suggested: '{}')",
            display(file),
            reason,
            suggested_filename
        ),
    }
}

/// Contextual one-line detail for a [`ValidationError`].
pub fn error_detail(error: &ValidationError) -> String {
    match error {
        ValidationError::BrokenPartOf { file, target } => {
            format!("{} → {} (target missing)", display(file), target)
        }
        ValidationError::BrokenContentsRef { index, target } => {
            format!("{} → {} (target missing)", display(index), target)
        }
        ValidationError::BrokenAttachment { file, attachment } => {
            format!("{} → {} (attachment missing)", display(file), attachment)
        }
        ValidationError::BrokenLinkRef { file, target } => {
            format!("{} → {} (link target missing)", display(file), target)
        }
    }
}
