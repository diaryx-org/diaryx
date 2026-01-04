//! Workspace link validation.
//!
//! This module provides functionality to validate `part_of` and `contents` references
//! within a workspace, detecting broken links and other structural issues.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::Result;
use crate::fs::FileSystem;
use crate::workspace::Workspace;

/// A validation error indicating a broken reference.
#[derive(Debug, Clone, Serialize)]
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
}

/// A validation warning indicating a potential issue.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum ValidationWarning {
    /// A file exists but is not referenced by any index's contents.
    OrphanFile {
        /// The orphan file path
        file: PathBuf,
    },
    /// A file or directory exists but is not in the contents hierarchy.
    /// Used for "List All Files" mode to show all filesystem entries.
    UnlinkedEntry {
        /// The entry path
        path: PathBuf,
        /// Whether this is a directory
        is_dir: bool,
    },
    /// Circular reference detected in workspace hierarchy.
    CircularReference {
        /// The files involved in the cycle
        files: Vec<PathBuf>,
    },
}

/// Result of validating a workspace.
#[derive(Debug, Clone, Default, Serialize)]
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
}

/// Validator for checking workspace link integrity.
pub struct Validator<FS: FileSystem> {
    ws: Workspace<FS>,
}

impl<FS: FileSystem> Validator<FS> {
    /// Create a new validator.
    pub fn new(fs: FS) -> Self {
        Self {
            ws: Workspace::new(fs),
        }
    }

    /// Validate all links starting from a workspace root index.
    ///
    /// Checks:
    /// - All `contents` references point to existing files
    /// - All `part_of` references point to existing files
    /// - Detects unlinked files/directories (not reachable via contents references)
    pub fn validate_workspace(&self, root_path: &Path) -> Result<ValidationResult> {
        let mut result = ValidationResult::default();
        let mut visited = HashSet::new();

        self.validate_recursive(root_path, &mut result, &mut visited)?;

        // Find unlinked entries: files/dirs in workspace not visited during traversal
        // Only scan immediate directory (non-recursive) for performance
        let workspace_root = root_path.parent().unwrap_or(Path::new("."));
        if let Ok(all_entries) = self.ws.fs_ref().list_files(workspace_root) {
            // Normalize visited paths for comparison
            let visited_normalized: HashSet<PathBuf> = visited
                .iter()
                .map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()))
                .collect();

            // Directories to skip (common build/dependency directories)
            let skip_dirs = [
                "node_modules",
                "target",
                ".git",
                ".svn",
                "dist",
                "build",
                "__pycache__",
                ".next",
                ".nuxt",
                "vendor",
                ".cargo",
            ];

            for entry in all_entries {
                // Skip entries in common non-workspace directories
                let should_skip = entry.components().any(|c| {
                    if let std::path::Component::Normal(name) = c {
                        skip_dirs.iter().any(|&d| name == std::ffi::OsStr::new(d))
                    } else {
                        false
                    }
                });

                if should_skip {
                    continue;
                }

                let entry_canonical = entry.canonicalize().unwrap_or_else(|_| entry.clone());
                if !visited_normalized.contains(&entry_canonical) {
                    let is_dir = self.ws.fs_ref().is_dir(&entry);

                    // Report as OrphanFile if it's an .md file (for backwards compat)
                    if entry.extension().is_some_and(|ext| ext == "md") {
                        result.warnings.push(ValidationWarning::OrphanFile {
                            file: entry.clone(),
                        });
                    }

                    // Always report as UnlinkedEntry for "List All Files" mode
                    result.warnings.push(ValidationWarning::UnlinkedEntry {
                        path: entry,
                        is_dir,
                    });
                }
            }
        }

        Ok(result)
    }

    /// Recursively validate from a given path.
    fn validate_recursive(
        &self,
        path: &Path,
        result: &mut ValidationResult,
        visited: &mut HashSet<PathBuf>,
    ) -> Result<()> {
        // Avoid cycles
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if visited.contains(&canonical) {
            result.warnings.push(ValidationWarning::CircularReference {
                files: vec![path.to_path_buf()],
            });
            return Ok(());
        }
        visited.insert(canonical);
        result.files_checked += 1;

        // Try to parse as index
        if let Ok(index) = self.ws.parse_index(path) {
            let dir = index.directory().unwrap_or_else(|| Path::new(""));

            // Check all contents references
            for child_ref in index.frontmatter.contents_list() {
                let child_path = dir.join(child_ref);

                if !self.ws.fs_ref().exists(&child_path) {
                    result.errors.push(ValidationError::BrokenContentsRef {
                        index: path.to_path_buf(),
                        target: child_ref.clone(),
                    });
                } else {
                    // Recurse into child
                    self.validate_recursive(&child_path, result, visited)?;
                }
            }

            // Check part_of if present
            if let Some(ref part_of) = index.frontmatter.part_of {
                let parent_path = dir.join(part_of);
                if !self.ws.fs_ref().exists(&parent_path) {
                    result.errors.push(ValidationError::BrokenPartOf {
                        file: path.to_path_buf(),
                        target: part_of.clone(),
                    });
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::MockFileSystem;

    #[test]
    fn test_valid_workspace() {
        let fs = MockFileSystem::new()
            .with_file(
                "README.md",
                "---\ntitle: Root\ncontents:\n  - note.md\n---\n",
            )
            .with_file("note.md", "---\ntitle: Note\npart_of: README.md\n---\n");

        let validator = Validator::new(fs);
        let result = validator
            .validate_workspace(Path::new("README.md"))
            .unwrap();

        assert!(result.is_ok());
        assert_eq!(result.files_checked, 2);
    }

    #[test]
    fn test_broken_contents_ref() {
        let fs = MockFileSystem::new().with_file(
            "README.md",
            "---\ntitle: Root\ncontents:\n  - missing.md\n---\n",
        );

        let validator = Validator::new(fs);
        let result = validator
            .validate_workspace(Path::new("README.md"))
            .unwrap();

        assert!(!result.is_ok());
        assert_eq!(result.errors.len(), 1);
        match &result.errors[0] {
            ValidationError::BrokenContentsRef { target, .. } => {
                assert_eq!(target, "missing.md");
            }
            _ => panic!("Expected BrokenContentsRef"),
        }
    }

    #[test]
    fn test_broken_part_of() {
        let fs = MockFileSystem::new()
            .with_file(
                "README.md",
                "---\ntitle: Root\ncontents:\n  - note.md\n---\n",
            )
            .with_file(
                "note.md",
                "---\ntitle: Note\npart_of: missing_parent.md\n---\n",
            );

        let validator = Validator::new(fs);
        let result = validator
            .validate_workspace(Path::new("README.md"))
            .unwrap();

        assert!(!result.is_ok());
        assert_eq!(result.errors.len(), 1);
        match &result.errors[0] {
            ValidationError::BrokenPartOf { target, .. } => {
                assert_eq!(target, "missing_parent.md");
            }
            _ => panic!("Expected BrokenPartOf"),
        }
    }
}
