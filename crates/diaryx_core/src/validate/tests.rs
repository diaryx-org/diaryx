//! Workspace validation test suite.
//!
//! Tests exercise the public surface via `super::*` which re-exports the
//! `ValidationError`, `ValidationWarning`, `ValidationResult`, `Validator`,
//! and `ValidationFixer` types. The tests survive the module split
//! transparently because they only ever touch re-exported symbols.

use super::*;
use crate::fs::{FileSystem, InMemoryFileSystem, SyncToAsyncFs, block_on_test};
use crate::link_parser::LinkFormat;
use std::path::{Path, PathBuf};

type TestFs = SyncToAsyncFs<InMemoryFileSystem>;

fn make_test_fs() -> InMemoryFileSystem {
    InMemoryFileSystem::new()
}

#[test]
fn test_valid_workspace() {
    let fs = make_test_fs();
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents:\n  - note.md\n---\n",
    )
    .unwrap();
    fs.write_file(
        Path::new("note.md"),
        "---\ntitle: Note\npart_of: README.md\n---\n",
    )
    .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    // Use None for unlimited depth in tests
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    assert!(result.is_ok());
    assert_eq!(result.files_checked, 2);
}

#[test]
fn test_broken_contents_ref() {
    let fs = make_test_fs();
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents:\n  - missing.md\n---\n",
    )
    .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

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
    let fs = make_test_fs();
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents:\n  - note.md\n---\n",
    )
    .unwrap();
    fs.write_file(
        Path::new("note.md"),
        "---\ntitle: Note\npart_of: missing_parent.md\n---\n",
    )
    .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    assert!(!result.is_ok());
    assert_eq!(result.errors.len(), 1);
    match &result.errors[0] {
        ValidationError::BrokenPartOf { target, .. } => {
            assert_eq!(target, "missing_parent.md");
        }
        _ => panic!("Expected BrokenPartOf"),
    }
}

#[test]
fn test_valid_self_link_passes_validation() {
    let fs = make_test_fs();
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\nlink: \"[Root](/README.md)\"\ncontents:\n  - note.md\n---\n",
    )
    .unwrap();
    fs.write_file(
        Path::new("note.md"),
        "---\ntitle: Note\nlink: \"[Note](/note.md)\"\npart_of: README.md\n---\n",
    )
    .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    assert!(result.is_ok());
    assert!(
        !result
            .warnings
            .iter()
            .any(|warning| matches!(warning, ValidationWarning::InvalidSelfLink { .. }))
    );
}

#[test]
fn test_invalid_self_link_warns_with_suggestion() {
    let fs = make_test_fs();
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents:\n  - note.md\n---\n",
    )
    .unwrap();
    fs.write_file(
        Path::new("note.md"),
        "---\ntitle: Note\nlink: \"[Wrong](/other.md)\"\npart_of: README.md\n---\n",
    )
    .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    let warning = result
        .warnings
        .iter()
        .find(|warning| matches!(warning, ValidationWarning::InvalidSelfLink { .. }))
        .expect("expected InvalidSelfLink warning");

    match warning {
        ValidationWarning::InvalidSelfLink {
            file,
            value,
            suggested,
        } => {
            assert_eq!(file, Path::new("note.md"));
            assert_eq!(value, "[Wrong](/other.md)");
            assert_eq!(suggested, "[Note](/note.md)");
        }
        other => panic!("expected InvalidSelfLink warning, got {:?}", other),
    }
}

#[test]
fn test_broken_link_ref_reports_error() {
    let fs = make_test_fs();
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\nlinks:\n  - missing.md\n---\n",
    )
    .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    assert!(result.errors.iter().any(|error| matches!(
        error,
        ValidationError::BrokenLinkRef { target, .. } if target == "missing.md"
    )));
}

#[test]
fn test_missing_backlink_warns() {
    let fs = make_test_fs();
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\nlinks:\n  - note.md\n---\n",
    )
    .unwrap();
    fs.write_file(Path::new("note.md"), "---\ntitle: Note\n---\n")
        .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    assert!(result.warnings.iter().any(|warning| matches!(
        warning,
        ValidationWarning::MissingBacklink { file, source, .. }
            if file == Path::new("note.md") && source == "README.md"
    )));
}

#[test]
fn test_stale_backlink_warns() {
    let fs = make_test_fs();
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\nlink_of:\n  - ghost.md\n---\n",
    )
    .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    assert!(result.warnings.iter().any(|warning| matches!(
        warning,
        ValidationWarning::StaleBacklink { file, value }
            if file == Path::new("README.md") && value == "ghost.md"
    )));
}

#[test]
fn test_missing_attachment_backlink_warns() {
    let fs = make_test_fs();
    // Root index references an attachment note, but the note is missing
    // `attachment_of` pointing back.
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents: []\nattachments:\n  - _attachments/pic.png.md\n---\n",
    )
    .unwrap();
    fs.write_file(
        Path::new("_attachments/pic.png.md"),
        "---\ntitle: pic\nattachment: _attachments/pic.png\n---\n",
    )
    .unwrap();
    fs.write_file(Path::new("_attachments/pic.png"), "PNGDATA")
        .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    assert!(
        result.warnings.iter().any(|warning| matches!(
            warning,
            ValidationWarning::MissingAttachmentBacklink { file, source, .. }
                if file == Path::new("_attachments/pic.png.md") && source == "README.md"
        )),
        "expected MissingAttachmentBacklink warning, got {:?}",
        result.warnings
    );
}

#[test]
fn test_stale_attachment_backlink_warns() {
    let fs = make_test_fs();
    // Attachment note declares `attachment_of: [ghost.md]`, but ghost.md
    // does not exist.
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents: []\n---\n",
    )
    .unwrap();
    fs.write_file(
        Path::new("_attachments/pic.png.md"),
        "---\ntitle: pic\nattachment: _attachments/pic.png\nattachment_of:\n  - ghost.md\n---\n",
    )
    .unwrap();
    fs.write_file(Path::new("_attachments/pic.png"), "PNGDATA")
        .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result =
        block_on_test(validator.validate_file(Path::new("_attachments/pic.png.md"))).unwrap();

    assert!(
        result.warnings.iter().any(|warning| matches!(
            warning,
            ValidationWarning::StaleAttachmentBacklink { file, value }
                if file == Path::new("_attachments/pic.png.md") && value == "ghost.md"
        )),
        "expected StaleAttachmentBacklink warning, got {:?}",
        result.warnings
    );
}

#[test]
fn test_detects_macos_absolute_path() {
    assert!(super::check::is_clearly_non_portable_path(
        "/Users/adam/Documents/file.md"
    ));
}

#[test]
fn test_detects_linux_absolute_path() {
    assert!(super::check::is_clearly_non_portable_path(
        "/home/user/diary/file.md"
    ));
}

#[test]
fn test_detects_linux_root_path() {
    assert!(super::check::is_clearly_non_portable_path("/root/file.md"));
}

#[test]
fn test_detects_windows_absolute_path_backslash() {
    assert!(super::check::is_clearly_non_portable_path(
        r"C:\Users\adam\Documents\file.md"
    ));
}

#[test]
fn test_detects_windows_absolute_path_forward_slash() {
    assert!(super::check::is_clearly_non_portable_path(
        "C:/Users/adam/Documents/file.md"
    ));
}

#[test]
fn test_allows_simple_relative_path() {
    assert!(!super::check::is_clearly_non_portable_path("../index.md"));
    assert!(!super::check::is_clearly_non_portable_path(
        "subdir/file.md"
    ));
    assert!(!super::check::is_clearly_non_portable_path("file.md"));
}

#[test]
fn test_allows_shallow_absolute_path() {
    // Paths with <= 4 components that don't match known patterns are allowed
    assert!(!super::check::is_clearly_non_portable_path("/data/file.md"));
}

#[test]
fn test_detects_deep_absolute_path() {
    // Deep absolute paths (>4 components) are flagged even without known patterns
    assert!(super::check::is_clearly_non_portable_path(
        "/some/deep/nested/path/file.md"
    ));
}

#[test]
fn test_compute_suggested_portable_path_for_absolute() {
    let base = Path::new("/workspace");
    let suggested =
        super::check::compute_suggested_portable_path("/Users/adam/Documents/file.md", base);
    assert_eq!(suggested, "file.md");
}

#[test]
fn test_compute_suggested_portable_path_for_relative_with_dots() {
    // Test that ./file.md gets normalized to file.md
    let base = Path::new("/workspace/subdir");
    let suggested = super::check::compute_suggested_portable_path("./file.md", base);
    assert_eq!(suggested, "file.md");

    // Test that subdir/../file.md gets normalized to file.md
    let suggested2 = super::check::compute_suggested_portable_path("subdir/../file.md", base);
    assert_eq!(suggested2, "file.md");
}

#[test]
fn test_compute_suggested_portable_path_same_directory() {
    // When target and source are in the same directory, should return just filename
    // Source: /workspace/Ideas/note.md (base_dir = /workspace/Ideas)
    // Target: /Users/adam/journal/Ideas/index.md
    // Result: index.md (same Ideas directory)
    let base = Path::new("/workspace/Ideas");
    let suggested =
        super::check::compute_suggested_portable_path("/Users/adam/journal/Ideas/index.md", base);
    assert_eq!(suggested, "index.md");
}

#[test]
fn test_compute_suggested_portable_path_parent_directory() {
    // When target is in parent directory relative to source
    // Source: /workspace/Ideas/SubFolder/note.md (base_dir = /workspace/Ideas/SubFolder)
    // Target: /Users/adam/journal/Ideas/index.md
    // Result: ../index.md
    let base = Path::new("/workspace/Ideas/SubFolder");
    let suggested =
        super::check::compute_suggested_portable_path("/Users/adam/journal/Ideas/index.md", base);
    assert_eq!(suggested, "../index.md");
}

#[test]
fn test_compute_suggested_portable_path_grandparent_directory() {
    // When target is in grandparent directory relative to source
    // Source: /workspace/Ideas/SubFolder/Deep/note.md (base_dir = /workspace/Ideas/SubFolder/Deep)
    // Target: /Users/adam/journal/Ideas/index.md
    // Result: ../../index.md
    let base = Path::new("/workspace/Ideas/SubFolder/Deep");
    let suggested =
        super::check::compute_suggested_portable_path("/Users/adam/journal/Ideas/index.md", base);
    assert_eq!(suggested, "../../index.md");
}

#[test]
fn test_compute_suggested_portable_path_sibling_directory() {
    // When target is in a sibling directory relative to source
    // Source: /workspace/Ideas/note.md (base_dir = /workspace/Ideas)
    // Target: /Users/adam/journal/Projects/index.md
    // Both Ideas and Projects are under journal, so result: ../Projects/index.md
    let base = Path::new("/workspace/journal/Ideas");
    let suggested = super::check::compute_suggested_portable_path(
        "/Users/adam/Documents/journal/Projects/index.md",
        base,
    );
    assert_eq!(suggested, "../Projects/index.md");
}

#[test]
fn test_validation_fixer_get_canonical_strips_corrupted_workspace_prefix() {
    let fs = make_test_fs();
    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let fixer = ValidationFixer::with_link_format(
        async_fs,
        PathBuf::from("/Users/test/workspace"),
        LinkFormat::default(),
    );

    let canonical = fixer.get_canonical(Path::new("Users/test/workspace/notes/day.md"));
    assert_eq!(canonical, "notes/day.md");
}

#[test]
fn test_non_portable_path_in_workspace_validation() {
    let fs = make_test_fs();
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents:\n  - note.md\n---\n",
    )
    .unwrap();
    fs.write_file(
        Path::new("note.md"),
        "---\ntitle: Note\npart_of: /Users/adam/Documents/README.md\n---\n",
    )
    .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    // Should have a warning (NonPortablePath), not an error (BrokenPartOf)
    assert!(result.is_ok()); // No errors
    assert_eq!(result.warnings.len(), 1);
    match &result.warnings[0] {
        ValidationWarning::NonPortablePath {
            property, value, ..
        } => {
            assert_eq!(property, "part_of");
            assert!(value.starts_with("/Users/"));
        }
        _ => panic!("Expected NonPortablePath warning"),
    }
}

#[test]
fn test_validate_workspace_with_plain_canonical_links() {
    // Create a workspace using PlainCanonical link format
    // PlainCanonical reads ambiguous plain paths as workspace-root.
    // Explicit relative paths (`./`, `../`) remain relative.
    let fs = make_test_fs();

    // Root index with link_format: plain_canonical
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\nlink_format: plain_canonical\ncontents:\n  - Folder/index.md\n---\n",
    )
    .unwrap();

    // Create directory structure
    fs.create_dir_all(Path::new("Folder")).unwrap();

    // Child index in Folder - uses file-relative paths
    fs.write_file(
        Path::new("Folder/index.md"),
        "---\ntitle: Folder Index\npart_of: ../README.md\ncontents:\n  - Folder/child.md\n---\n",
    )
    .unwrap();

    // Child file - canonical part_of path
    fs.write_file(
        Path::new("Folder/child.md"),
        "---\ntitle: Child\npart_of: Folder/index.md\n---\n",
    )
    .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    // Should have no errors - file-relative paths should resolve correctly
    assert!(
        result.errors.is_empty(),
        "Expected no errors, got: {:?}",
        result.errors
    );
    assert_eq!(result.files_checked, 3);
}

#[test]
fn test_validate_workspace_plain_canonical_deeply_nested() {
    // Test deeply nested workspace with PlainCanonical references.
    let fs = make_test_fs();

    // Root with PlainCanonical format setting
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\nlink_format: plain_canonical\ncontents:\n  - A/index.md\n---\n",
    )
    .unwrap();

    fs.create_dir_all(Path::new("A/B")).unwrap();

    // A/index.md uses canonical contents path
    fs.write_file(
        Path::new("A/index.md"),
        "---\ntitle: A\npart_of: ../README.md\ncontents:\n  - A/B/index.md\n---\n",
    )
    .unwrap();

    // A/B/index.md uses canonical contents path
    fs.write_file(
        Path::new("A/B/index.md"),
        "---\ntitle: B\npart_of: ../index.md\ncontents:\n  - A/B/note.md\n---\n",
    )
    .unwrap();

    // Leaf file uses canonical part_of path
    fs.write_file(
        Path::new("A/B/note.md"),
        "---\ntitle: Note\npart_of: A/B/index.md\n---\n",
    )
    .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    // Should have no errors
    assert!(
        result.errors.is_empty(),
        "Expected no errors, got: {:?}",
        result.errors
    );
    assert_eq!(result.files_checked, 4);
}

#[test]
fn test_validate_workspace_with_markdown_root_and_plain_paths() {
    // Test MarkdownRoot format with explicit markdown links
    // Ambiguous plain paths resolve as file-relative for backwards compatibility.
    let fs = make_test_fs();

    // Root index with link_format: markdown_root and proper markdown link in contents
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\nlink_format: markdown_root\ncontents:\n  - \"[Folder Index](/Folder/index.md)\"\n---\n",
    )
    .unwrap();

    fs.create_dir_all(Path::new("Folder")).unwrap();

    // Child index using file-relative path for part_of
    // (The proper MarkdownRoot format would be "[Root](/README.md)" but plain
    // relative paths are also supported for backwards compatibility)
    fs.write_file(
        Path::new("Folder/index.md"),
        "---\ntitle: Folder Index\npart_of: ../README.md\ncontents: []\n---\n",
    )
    .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    // Should have no errors
    assert!(
        result.errors.is_empty(),
        "Expected no errors with MarkdownRoot format, got: {:?}",
        result.errors
    );
    assert_eq!(result.files_checked, 2);
}

#[test]
fn test_validate_workspace_with_actual_markdown_links_in_contents() {
    // Test that markdown links with [Title](/path) syntax work correctly
    // This is the format users typically see in their files
    let fs = make_test_fs();

    // Root index with link_format: markdown_root and actual markdown links in contents
    fs.write_file(
        Path::new("README.md"),
        r#"---
title: Root
link_format: markdown_root
contents:
  - "[Daily Index](/Daily/daily_index.md)"
  - "[Creative Writing](</Creative Writing/index.md>)"
---
"#,
    )
    .unwrap();

    // Create directories
    fs.create_dir_all(Path::new("Daily")).unwrap();
    fs.create_dir_all(Path::new("Creative Writing")).unwrap();

    // Create the referenced files
    fs.write_file(
        Path::new("Daily/daily_index.md"),
        "---\ntitle: Daily Index\npart_of: \"[Root](/README.md)\"\n---\n",
    )
    .unwrap();

    fs.write_file(
        Path::new("Creative Writing/index.md"),
        "---\ntitle: Creative Writing\npart_of: \"[Root](/README.md)\"\n---\n",
    )
    .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    // Should have no errors - markdown links should parse and resolve correctly
    assert!(
        result.errors.is_empty(),
        "Expected no errors with markdown links, got: {:?}",
        result.errors
    );
    assert_eq!(result.files_checked, 3);
}

#[test]
fn test_validate_workspace_with_relative_format_resolves_ambiguous_relatively() {
    // Test that with PlainRelative format, ambiguous paths resolve relative to current file
    let fs = make_test_fs();

    // Root WITH link_format: plain_relative (ambiguous paths should resolve relatively)
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\nlink_format: plain_relative\ncontents:\n  - Folder/index.md\n---\n",
    )
    .unwrap();

    fs.create_dir_all(Path::new("Folder")).unwrap();

    // Child that uses ambiguous path for part_of
    // With plain_relative, this will resolve to Folder/README.md which doesn't exist
    fs.write_file(
        Path::new("Folder/index.md"),
        "---\ntitle: Folder Index\npart_of: README.md\ncontents: []\n---\n",
    )
    .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    // Should have an error - "README.md" resolves to "Folder/README.md" (relative)
    // which doesn't exist, because plain_relative treats ambiguous as relative
    assert!(
        !result.errors.is_empty(),
        "Expected errors for ambiguous paths with PlainRelative format"
    );
    match &result.errors[0] {
        ValidationError::BrokenPartOf { target, .. } => {
            assert_eq!(target, "README.md");
        }
        _ => panic!("Expected BrokenPartOf error"),
    }
}

#[test]
fn test_validate_workspace_default_format_with_file_relative_paths() {
    // Test that ambiguous paths resolve as file-relative for backwards compatibility.
    // This supports legacy workspaces that use file-relative paths.
    let fs = make_test_fs();

    // Root WITHOUT explicit link_format (defaults to MarkdownRoot)
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents:\n  - Folder/index.md\n---\n",
    )
    .unwrap();

    fs.create_dir_all(Path::new("Folder")).unwrap();

    // Child uses file-relative path for part_of
    fs.write_file(
        Path::new("Folder/index.md"),
        "---\ntitle: Folder Index\npart_of: ../README.md\ncontents: []\n---\n",
    )
    .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    // Should have NO errors - file-relative paths are supported for backwards compatibility
    assert!(
        result.errors.is_empty(),
        "Expected no errors with default format, got: {:?}",
        result.errors
    );
    assert_eq!(result.files_checked, 2);
}

#[test]
fn test_exclude_patterns_suppress_orphan_binary_warnings() {
    // Test that exclude patterns suppress OrphanBinaryFile warnings
    let fs = make_test_fs();

    // Root with exclude patterns
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents: []\nexclude:\n  - \"*.lock\"\n  - \"*.toml\"\n---\n",
    )
    .unwrap();

    // Create some files that should be excluded
    fs.write_file(Path::new("Cargo.lock"), "# lock file")
        .unwrap();
    fs.write_file(Path::new("Cargo.toml"), "[package]").unwrap();

    // Create a file that should NOT be excluded
    fs.write_file(Path::new("config.json"), "{}").unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    // Check warnings - should only have OrphanBinaryFile for config.json
    let orphan_warnings: Vec<_> = result
        .warnings
        .iter()
        .filter_map(|w| {
            if let ValidationWarning::OrphanBinaryFile { file, .. } = w {
                Some(file.file_name()?.to_str()?.to_string())
            } else {
                None
            }
        })
        .collect();

    assert!(
        !orphan_warnings.contains(&"Cargo.lock".to_string()),
        "Cargo.lock should be excluded, got warnings: {:?}",
        orphan_warnings
    );
    assert!(
        !orphan_warnings.contains(&"Cargo.toml".to_string()),
        "Cargo.toml should be excluded, got warnings: {:?}",
        orphan_warnings
    );
    assert!(
        orphan_warnings.contains(&"config.json".to_string()),
        "config.json should trigger a warning, got warnings: {:?}",
        orphan_warnings
    );
}

#[test]
fn test_root_index_excludes_apply_even_with_sibling_leaf_markdown_files() {
    let fs = make_test_fs();

    fs.write_file(
        Path::new("Diaryx.md"),
        "---\ntitle: Root\ncontents:\n  - AGENTS.md\nexclude:\n  - \"*.toml\"\n  - \"*.lock\"\n---\n",
    )
    .unwrap();
    fs.write_file(
        Path::new("AGENTS.md"),
        "---\ntitle: Agents\npart_of: Diaryx.md\n---\n",
    )
    .unwrap();

    fs.write_file(Path::new("Cargo.toml"), "[package]").unwrap();
    fs.write_file(Path::new("flake.lock"), "lock").unwrap();
    fs.write_file(Path::new("config.json"), "{}").unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("Diaryx.md"), None)).unwrap();

    let orphan_binaries: Vec<_> = result
        .warnings
        .iter()
        .filter_map(|w| {
            if let ValidationWarning::OrphanBinaryFile { file, .. } = w {
                Some(file.file_name()?.to_str()?.to_string())
            } else {
                None
            }
        })
        .collect();

    assert!(
        !orphan_binaries.contains(&"Cargo.toml".to_string()),
        "Cargo.toml should inherit root excludes, got warnings: {:?}",
        orphan_binaries
    );
    assert!(
        !orphan_binaries.contains(&"flake.lock".to_string()),
        "flake.lock should inherit root excludes, got warnings: {:?}",
        orphan_binaries
    );
    assert!(
        orphan_binaries.contains(&"config.json".to_string()),
        "config.json should still warn, got warnings: {:?}",
        orphan_binaries
    );
}

#[test]
fn test_validate_workspace_prunes_excluded_directories_during_scan() {
    let fs = make_test_fs();

    fs.create_dir_all(Path::new("build/nested")).unwrap();

    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents: []\nexclude:\n  - \"build/**\"\n---\n",
    )
    .unwrap();
    fs.write_file(Path::new("build/output.json"), "{}").unwrap();
    fs.write_file(Path::new("build/nested/extra.bin"), "bin")
        .unwrap();
    fs.write_file(Path::new("visible.json"), "{}").unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    let orphan_binaries: Vec<_> = result
        .warnings
        .iter()
        .filter_map(|w| {
            if let ValidationWarning::OrphanBinaryFile { file, .. } = w {
                Some(file.to_string_lossy().to_string())
            } else {
                None
            }
        })
        .collect();

    assert!(
        !orphan_binaries
            .iter()
            .any(|path| path.starts_with("build/")),
        "build/** should prune the directory scan, got warnings: {:?}",
        orphan_binaries
    );
    assert!(
        orphan_binaries.contains(&"visible.json".to_string()),
        "visible.json should still warn, got warnings: {:?}",
        orphan_binaries
    );
}

#[test]
fn test_validate_workspace_matches_excludes_against_workspace_relative_paths() {
    let fs = make_test_fs();

    fs.create_dir_all(Path::new("crates/diaryx")).unwrap();

    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents: []\nexclude:\n  - \"**/target\"\n  - \"**/target/**\"\n---\n",
    )
    .unwrap();
    fs.write_file(Path::new("crates/diaryx/target/app.bin"), "bin")
        .unwrap();
    fs.write_file(Path::new("crates/diaryx/kept.bin"), "bin")
        .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    let orphan_binaries: Vec<_> = result
        .warnings
        .iter()
        .filter_map(|w| {
            if let ValidationWarning::OrphanBinaryFile { file, .. } = w {
                Some(file.to_string_lossy().to_string())
            } else {
                None
            }
        })
        .collect();

    assert!(
        !orphan_binaries.contains(&"crates/diaryx/target/app.bin".to_string()),
        "workspace-relative excludes should suppress nested target paths, got warnings: {:?}",
        orphan_binaries
    );
    assert!(
        orphan_binaries.contains(&"crates/diaryx/kept.bin".to_string()),
        "non-excluded sibling should still warn, got warnings: {:?}",
        orphan_binaries
    );
}

#[test]
fn test_validate_workspace_prunes_builtin_skip_directories_during_scan() {
    let fs = make_test_fs();

    fs.create_dir_all(Path::new("target/debug")).unwrap();
    fs.create_dir_all(Path::new("node_modules/pkg")).unwrap();

    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents: []\n---\n",
    )
    .unwrap();
    fs.write_file(Path::new("target/debug/app.bin"), "bin")
        .unwrap();
    fs.write_file(Path::new("node_modules/pkg/index.js"), "js")
        .unwrap();
    fs.write_file(Path::new("visible.json"), "{}").unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    let orphan_paths: Vec<_> = result
        .warnings
        .iter()
        .filter_map(|warning| match warning {
            ValidationWarning::OrphanBinaryFile { file, .. } => {
                Some(file.to_string_lossy().to_string())
            }
            _ => None,
        })
        .collect();

    assert!(
        !orphan_paths.iter().any(|path| path.starts_with("target/")),
        "target should be pruned before traversal, got warnings: {:?}",
        orphan_paths
    );
    assert!(
        !orphan_paths
            .iter()
            .any(|path| path.starts_with("node_modules/")),
        "node_modules should be pruned before traversal, got warnings: {:?}",
        orphan_paths
    );
    assert!(
        orphan_paths.contains(&"visible.json".to_string()),
        "visible.json should still warn, got warnings: {:?}",
        orphan_paths
    );
}

#[test]
fn test_validate_workspace_prunes_hidden_directories_during_scan() {
    let fs = make_test_fs();

    fs.create_dir_all(Path::new(".direnv/cache")).unwrap();
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents: []\n---\n",
    )
    .unwrap();
    fs.write_file(Path::new(".direnv/cache/stale.bin"), "bin")
        .unwrap();
    fs.write_file(Path::new("visible.json"), "{}").unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    let orphan_paths: Vec<_> = result
        .warnings
        .iter()
        .filter_map(|warning| match warning {
            ValidationWarning::OrphanBinaryFile { file, .. } => {
                Some(file.to_string_lossy().to_string())
            }
            _ => None,
        })
        .collect();

    assert!(
        !orphan_paths.iter().any(|path| path.starts_with(".direnv/")),
        "hidden directories should be pruned before traversal, got warnings: {:?}",
        orphan_paths
    );
    assert!(
        orphan_paths.contains(&"visible.json".to_string()),
        "visible.json should still warn, got warnings: {:?}",
        orphan_paths
    );
}

#[test]
fn test_exclude_patterns_suppress_unlisted_markdown_warnings() {
    // Test that exclude patterns suppress OrphanFile warnings for markdown files
    let fs = make_test_fs();

    // Create a subdirectory structure - OrphanFile warnings are generated
    // when scanning files in the same directory as an index file
    fs.create_dir_all(Path::new("docs")).unwrap();

    // Root index that lists the docs folder
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents:\n  - docs/README.md\n---\n",
    )
    .unwrap();

    // Docs index with exclude patterns and one listed file
    fs.write_file(
        Path::new("docs/README.md"),
        "---\ntitle: Docs\npart_of: ../README.md\ncontents:\n  - included.md\nexclude:\n  - \"LICENSE.md\"\n  - \"CHANGELOG.md\"\n---\n",
    )
    .unwrap();

    // Create the included file (so it's listed in contents)
    fs.write_file(
        Path::new("docs/included.md"),
        "---\ntitle: Included\npart_of: README.md\n---\n# Included",
    )
    .unwrap();

    // Create some markdown files that should be excluded
    fs.write_file(
        Path::new("docs/LICENSE.md"),
        "---\ntitle: License\n---\n# License",
    )
    .unwrap();
    fs.write_file(
        Path::new("docs/CHANGELOG.md"),
        "---\ntitle: Changelog\n---\n# Changelog",
    )
    .unwrap();

    // Create a markdown file that should NOT be excluded
    fs.write_file(
        Path::new("docs/notes.md"),
        "---\ntitle: Notes\n---\n# Notes",
    )
    .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    // Check warnings - should only have OrphanFile for notes.md
    let orphan_warnings: Vec<_> = result
        .warnings
        .iter()
        .filter_map(|w| {
            if let ValidationWarning::OrphanFile { file, .. } = w {
                Some(file.file_name()?.to_str()?.to_string())
            } else {
                None
            }
        })
        .collect();

    assert!(
        !orphan_warnings.contains(&"LICENSE.md".to_string()),
        "LICENSE.md should be excluded, got warnings: {:?}",
        orphan_warnings
    );
    assert!(
        !orphan_warnings.contains(&"CHANGELOG.md".to_string()),
        "CHANGELOG.md should be excluded, got warnings: {:?}",
        orphan_warnings
    );
    assert!(
        orphan_warnings.contains(&"notes.md".to_string()),
        "notes.md should trigger a warning, got warnings: {:?}",
        orphan_warnings
    );
}

#[test]
fn test_validate_workspace_missing_part_of_in_contents() {
    // A file listed in contents but missing part_of should produce MissingPartOf warning
    let fs = make_test_fs();
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents:\n  - note.md\n---\n",
    )
    .unwrap();
    // note.md has no part_of property
    fs.write_file(Path::new("note.md"), "---\ntitle: Note\n---\n")
        .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    let missing_part_of: Vec<_> = result
        .warnings
        .iter()
        .filter(|w| matches!(w, ValidationWarning::MissingPartOf { .. }))
        .collect();

    assert_eq!(
        missing_part_of.len(),
        1,
        "Expected 1 MissingPartOf warning, got: {:?}",
        missing_part_of
    );
    match &missing_part_of[0] {
        ValidationWarning::MissingPartOf { file, .. } => {
            assert_eq!(file, Path::new("note.md"));
        }
        _ => unreachable!(),
    }
}

#[test]
fn test_validate_workspace_no_missing_part_of_for_root() {
    // The root index should NOT produce a MissingPartOf warning
    let fs = make_test_fs();
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents:\n  - note.md\n---\n",
    )
    .unwrap();
    fs.write_file(
        Path::new("note.md"),
        "---\ntitle: Note\npart_of: README.md\n---\n",
    )
    .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    let missing_part_of: Vec<_> = result
        .warnings
        .iter()
        .filter(|w| matches!(w, ValidationWarning::MissingPartOf { .. }))
        .collect();

    assert!(
        missing_part_of.is_empty(),
        "Root index should not get MissingPartOf, got: {:?}",
        missing_part_of
    );
}

#[test]
fn test_validate_workspace_no_missing_part_of_for_sub_index() {
    // A sub-index (has contents) without part_of should NOT produce MissingPartOf
    // since it could be a valid sub-root
    let fs = make_test_fs();
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents:\n  - sub/index.md\n---\n",
    )
    .unwrap();
    fs.create_dir_all(Path::new("sub")).unwrap();
    // sub/index.md has contents but no part_of
    fs.write_file(
        Path::new("sub/index.md"),
        "---\ntitle: Sub\ncontents:\n  - child.md\n---\n",
    )
    .unwrap();
    fs.write_file(
        Path::new("sub/child.md"),
        "---\ntitle: Child\npart_of: index.md\n---\n",
    )
    .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    let missing_part_of: Vec<_> = result
        .warnings
        .iter()
        .filter(|w| matches!(w, ValidationWarning::MissingPartOf { .. }))
        .collect();

    assert!(
        missing_part_of.is_empty(),
        "Sub-index without part_of should not get MissingPartOf, got: {:?}",
        missing_part_of
    );
}

#[test]
fn test_validate_workspace_orphan_file_missing_part_of() {
    // An orphan markdown file (not in any contents) without part_of should produce
    // both OrphanFile and MissingPartOf warnings
    let fs = make_test_fs();
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents:\n  - note.md\n---\n",
    )
    .unwrap();
    fs.write_file(
        Path::new("note.md"),
        "---\ntitle: Note\npart_of: README.md\n---\n",
    )
    .unwrap();
    // orphan.md is not listed in any contents and has no part_of
    fs.write_file(Path::new("orphan.md"), "---\ntitle: Orphan\n---\n")
        .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    let orphan_warnings: Vec<_> = result
        .warnings
        .iter()
        .filter(|w| matches!(w, ValidationWarning::OrphanFile { .. }))
        .collect();
    let missing_part_of: Vec<_> = result
        .warnings
        .iter()
        .filter(|w| matches!(w, ValidationWarning::MissingPartOf { .. }))
        .collect();

    assert_eq!(
        orphan_warnings.len(),
        1,
        "Expected 1 OrphanFile warning, got: {:?}",
        orphan_warnings
    );
    assert_eq!(
        missing_part_of.len(),
        1,
        "Expected 1 MissingPartOf warning for orphan, got: {:?}",
        missing_part_of
    );
}

#[test]
fn test_attachment_notes_excluded_from_contents_part_of_validation() {
    // Attachment notes (files with the `attachment` property) should not produce
    // OrphanFile or MissingPartOf warnings - they are managed via `attachments` lists.
    let fs = make_test_fs();
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents:\n  - note.md\n---\n",
    )
    .unwrap();
    fs.write_file(
        Path::new("note.md"),
        "---\ntitle: Note\npart_of: README.md\n---\n",
    )
    .unwrap();
    // This is an attachment note - it has the `attachment` property
    fs.write_file(
        Path::new("_attachments/photo.md"),
        "---\ntitle: Photo\nattachment: photo.jpg\n---\n",
    )
    .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    let orphan_warnings: Vec<_> = result
        .warnings
        .iter()
        .filter(|w| matches!(w, ValidationWarning::OrphanFile { .. }))
        .collect();
    let missing_part_of: Vec<_> = result
        .warnings
        .iter()
        .filter(|w| matches!(w, ValidationWarning::MissingPartOf { .. }))
        .collect();

    assert_eq!(
        orphan_warnings.len(),
        0,
        "Attachment notes should not produce OrphanFile warnings, got: {:?}",
        orphan_warnings
    );
    assert_eq!(
        missing_part_of.len(),
        0,
        "Attachment notes should not produce MissingPartOf warnings, got: {:?}",
        missing_part_of
    );
}

#[test]
fn test_attachment_binary_not_reported_as_orphan() {
    // A binary file wrapped in an attachment note (referenced via the note's
    // `attachment:` property) should not be flagged as OrphanBinaryFile.
    let fs = make_test_fs();
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents:\n  - note.md\n---\n",
    )
    .unwrap();
    fs.write_file(
        Path::new("note.md"),
        "---\ntitle: Note\npart_of: README.md\nattachments:\n  - _attachments/photo.jpg.md\n---\n",
    )
    .unwrap();
    fs.write_file(
        Path::new("_attachments/photo.jpg.md"),
        "---\ntitle: Photo\nattachment: photo.jpg\n---\n",
    )
    .unwrap();
    fs.write_file(Path::new("_attachments/photo.jpg"), "binary content")
        .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    let orphan_binaries: Vec<_> = result
        .warnings
        .iter()
        .filter(|w| matches!(w, ValidationWarning::OrphanBinaryFile { .. }))
        .collect();

    assert!(
        orphan_binaries.is_empty(),
        "Binary wrapped in attachment note should not be an orphan, got: {:?}",
        orphan_binaries
    );
    assert!(
        result.errors.is_empty(),
        "Should have no errors, got: {:?}",
        result.errors
    );
}

#[test]
fn test_missing_attachment_produces_broken_attachment_error() {
    // validate_workspace should flag an attachments entry that doesn't exist,
    // matching validate_file's behavior.
    let fs = make_test_fs();
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\nattachments:\n  - _attachments/missing.md\n---\n",
    )
    .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    assert!(
        result
            .errors
            .iter()
            .any(|e| matches!(e, ValidationError::BrokenAttachment { .. })),
        "Expected BrokenAttachment error, got: {:?}",
        result.errors
    );
}

#[test]
fn test_attachments_raw_binary_entry_is_flagged() {
    // Real-world case: an index lists a `.HEIC` directly in `attachments`
    // (legacy flat format). The new model requires a markdown attachment
    // note, so this should produce an InvalidAttachmentRef warning.
    let fs = make_test_fs();
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents:\n  - pictures.md\n---\n",
    )
    .unwrap();
    fs.write_file(
        Path::new("pictures.md"),
        "---\ntitle: Pictures\npart_of: README.md\ncontents: []\nattachments:\n  - photo.HEIC\n---\n",
    )
    .unwrap();
    fs.write_file(Path::new("photo.HEIC"), "binary content")
        .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    let invalid: Vec<_> = result
        .warnings
        .iter()
        .filter(|w| matches!(w, ValidationWarning::InvalidAttachmentRef { .. }))
        .collect();
    assert_eq!(
        invalid.len(),
        1,
        "Expected one InvalidAttachmentRef warning, got: {:?}",
        result.warnings
    );
    match invalid[0] {
        ValidationWarning::InvalidAttachmentRef { target, .. } => {
            assert_eq!(target, "photo.HEIC");
        }
        _ => unreachable!(),
    }
    // No BrokenAttachment since the binary exists on disk.
    assert!(
        result.errors.is_empty(),
        "Should have no errors, got: {:?}",
        result.errors
    );
}

#[test]
fn test_attachments_markdown_without_attachment_prop_is_flagged() {
    // An index lists a .md file in attachments, but that file is a regular
    // note — it has no `attachment:` property — so it's not a valid
    // attachment note. Must produce InvalidAttachmentRef.
    let fs = make_test_fs();
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents:\n  - pictures.md\n---\n",
    )
    .unwrap();
    fs.write_file(
        Path::new("pictures.md"),
        "---\ntitle: Pictures\npart_of: README.md\ncontents: []\nattachments:\n  - notes.md\n---\n",
    )
    .unwrap();
    fs.write_file(
        Path::new("notes.md"),
        "---\ntitle: Notes\n---\nJust a regular note, not an attachment note.\n",
    )
    .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    assert!(
        result
            .warnings
            .iter()
            .any(|w| matches!(w, ValidationWarning::InvalidAttachmentRef { .. })),
        "Expected InvalidAttachmentRef warning, got: {:?}",
        result.warnings
    );
}

#[test]
fn test_attachments_valid_attachment_note_no_warning() {
    // Positive control: a proper attachment note (.md with `attachment:`
    // pointing at a binary) must NOT trigger InvalidAttachmentRef.
    let fs = make_test_fs();
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents:\n  - pictures.md\n---\n",
    )
    .unwrap();
    fs.write_file(
        Path::new("pictures.md"),
        "---\ntitle: Pictures\npart_of: README.md\ncontents: []\nattachments:\n  - _attachments/photo.HEIC.md\n---\n",
    )
    .unwrap();
    fs.write_file(
        Path::new("_attachments/photo.HEIC.md"),
        "---\ntitle: photo.HEIC\nattachment: photo.HEIC\n---\n",
    )
    .unwrap();
    fs.write_file(Path::new("_attachments/photo.HEIC"), "binary content")
        .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    let invalid: Vec<_> = result
        .warnings
        .iter()
        .filter(|w| matches!(w, ValidationWarning::InvalidAttachmentRef { .. }))
        .collect();
    assert!(
        invalid.is_empty(),
        "Valid attachment note should not produce InvalidAttachmentRef, got: {:?}",
        invalid
    );
}

#[test]
fn test_duplicate_list_entry_string_equality() {
    // Real-world case from pictures_index.md: the same exact value appears
    // multiple times in `attachments`. Emit one DuplicateListEntry warning
    // per distinct duplicated value, not one per duplicate occurrence.
    let fs = make_test_fs();
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents:\n  - pictures.md\n---\n",
    )
    .unwrap();
    fs.write_file(
        Path::new("pictures.md"),
        "---\ntitle: Pictures\npart_of: README.md\ncontents: []\n\
         attachments:\n  - _attachments/a.md\n  - _attachments/b.md\n  \
         - _attachments/a.md\n  - _attachments/b.md\n  - _attachments/a.md\n---\n",
    )
    .unwrap();
    fs.write_file(
        Path::new("_attachments/a.md"),
        "---\ntitle: a\nattachment: a.jpg\n---\n",
    )
    .unwrap();
    fs.write_file(
        Path::new("_attachments/b.md"),
        "---\ntitle: b\nattachment: b.jpg\n---\n",
    )
    .unwrap();
    fs.write_file(Path::new("_attachments/a.jpg"), "binary")
        .unwrap();
    fs.write_file(Path::new("_attachments/b.jpg"), "binary")
        .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    let dupes: Vec<_> = result
        .warnings
        .iter()
        .filter_map(|w| match w {
            ValidationWarning::DuplicateListEntry {
                property,
                value,
                count,
                ..
            } => Some((property.as_str(), value.as_str(), *count)),
            _ => None,
        })
        .collect();

    assert_eq!(dupes.len(), 2, "Expected 2 dupes, got: {:?}", dupes);
    assert!(
        dupes
            .iter()
            .any(|(p, v, c)| *p == "attachments" && *v == "_attachments/a.md" && *c == 3)
    );
    assert!(
        dupes
            .iter()
            .any(|(p, v, c)| *p == "attachments" && *v == "_attachments/b.md" && *c == 2)
    );
}

#[test]
fn test_duplicate_list_entry_canonical_equivalence() {
    // Two entries written in different shapes (`foo.md`, `[Foo](./foo.md)`,
    // `./foo.md`) all resolve to the same canonical path and must count
    // as duplicates of one another.
    let fs = make_test_fs();
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents:\n  - foo.md\n  - '[Foo](./foo.md)'\n  - ./foo.md\n---\n",
    )
    .unwrap();
    fs.write_file(
        Path::new("foo.md"),
        "---\ntitle: Foo\npart_of: README.md\n---\n",
    )
    .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    let dupes: Vec<_> = result
        .warnings
        .iter()
        .filter(|w| matches!(w, ValidationWarning::DuplicateListEntry { .. }))
        .collect();
    assert_eq!(
        dupes.len(),
        1,
        "Expected one DuplicateListEntry warning across equivalent shapes, got: {:?}",
        dupes
    );
    match dupes[0] {
        ValidationWarning::DuplicateListEntry {
            property,
            value,
            count,
            ..
        } => {
            assert_eq!(property, "contents");
            assert_eq!(value, "foo.md"); // First occurrence kept as shown
            assert_eq!(*count, 3);
        }
        _ => unreachable!(),
    }
}

#[test]
fn test_no_duplicate_warning_for_unique_entries() {
    // Sanity check: a list with no duplicates produces no warning.
    let fs = make_test_fs();
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents:\n  - a.md\n  - b.md\n  - c.md\n---\n",
    )
    .unwrap();
    fs.write_file(
        Path::new("a.md"),
        "---\ntitle: a\npart_of: README.md\n---\n",
    )
    .unwrap();
    fs.write_file(
        Path::new("b.md"),
        "---\ntitle: b\npart_of: README.md\n---\n",
    )
    .unwrap();
    fs.write_file(
        Path::new("c.md"),
        "---\ntitle: c\npart_of: README.md\n---\n",
    )
    .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    let has_dupe = result
        .warnings
        .iter()
        .any(|w| matches!(w, ValidationWarning::DuplicateListEntry { .. }));
    assert!(!has_dupe, "Unique list should not emit DuplicateListEntry");
}

#[test]
fn test_fix_duplicate_list_entry_dedupes_attachments() {
    // The auto-fix should preserve the first occurrence of each canonical
    // value and strip the rest, leaving a clean list.
    let fs = make_test_fs();
    fs.write_file(
        Path::new("pictures.md"),
        "---\ntitle: Pictures\ncontents: []\n\
         attachments:\n  - _attachments/a.md\n  - _attachments/b.md\n  \
         - _attachments/a.md\n  - _attachments/b.md\n---\n",
    )
    .unwrap();
    fs.write_file(
        Path::new("_attachments/a.md"),
        "---\ntitle: a\nattachment: a.jpg\n---\n",
    )
    .unwrap();
    fs.write_file(
        Path::new("_attachments/b.md"),
        "---\ntitle: b\nattachment: b.jpg\n---\n",
    )
    .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs.clone());
    let fixer = ValidationFixer::new(async_fs);
    let fix_result =
        block_on_test(fixer.fix_duplicate_list_entry(Path::new("pictures.md"), "attachments"));
    assert!(fix_result.success, "fix failed: {}", fix_result.message);

    // Re-parse and confirm the list is now just the two unique entries.
    let content = fs.read_to_string(Path::new("pictures.md")).unwrap();
    let parsed = crate::frontmatter::parse_or_empty(&content).unwrap();
    let attachments = crate::frontmatter::get_string_array(&parsed.frontmatter, "attachments");
    assert_eq!(
        attachments,
        vec![
            "_attachments/a.md".to_string(),
            "_attachments/b.md".to_string()
        ],
        "Expected deduped list preserving first occurrences"
    );
}

#[test]
fn test_temp_files_excluded_from_validation() {
    // Temp files (.bak, .tmp, .swap) from atomic writes should not produce warnings
    let fs = make_test_fs();

    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents: []\n---\n",
    )
    .unwrap();

    // Create temp files that should be silently ignored
    fs.write_file(Path::new("file.md.bak"), "backup content")
        .unwrap();
    fs.write_file(Path::new("file.md.tmp"), "temp content")
        .unwrap();
    fs.write_file(Path::new("file.md.swap"), "swap content")
        .unwrap();

    // Create a real orphan for contrast
    fs.write_file(Path::new("orphan.txt"), "real orphan")
        .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    let all_warning_files: Vec<String> = result
        .warnings
        .iter()
        .filter_map(|w| {
            w.file_path()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
        })
        .collect();

    assert!(
        !all_warning_files.iter().any(|f| f.ends_with(".bak")),
        ".bak files should be excluded, got warnings: {:?}",
        all_warning_files
    );
    assert!(
        !all_warning_files.iter().any(|f| f.ends_with(".tmp")),
        ".tmp files should be excluded, got warnings: {:?}",
        all_warning_files
    );
    assert!(
        !all_warning_files.iter().any(|f| f.ends_with(".swap")),
        ".swap files should be excluded, got warnings: {:?}",
        all_warning_files
    );
    assert!(
        all_warning_files.contains(&"orphan.txt".to_string()),
        "Real orphan should still produce a warning, got: {:?}",
        all_warning_files
    );
}

#[test]
fn test_multiple_indexes_detection_is_content_based() {
    // MultipleIndexes should flag any .md file in the dir whose frontmatter
    // has `contents:`, regardless of filename. A `README.md` WITHOUT contents
    // is not an index; a `journal.md` WITH contents is.
    let fs = make_test_fs();
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents:\n  - journal.md\n---\n",
    )
    .unwrap();
    // journal.md is a real index (has `contents:`) even though its filename
    // doesn't match README/index/*.index.md.
    fs.write_file(
        Path::new("journal.md"),
        "---\ntitle: Journal\ncontents: []\npart_of: README.md\n---\n",
    )
    .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_file(Path::new("README.md"))).unwrap();

    let multi: Vec<_> = result
        .warnings
        .iter()
        .filter_map(|w| match w {
            ValidationWarning::MultipleIndexes { indexes, .. } => Some(indexes),
            _ => None,
        })
        .collect();

    assert_eq!(
        multi.len(),
        1,
        "Expected one MultipleIndexes warning, got {:?}",
        result.warnings
    );
    assert_eq!(multi[0].len(), 2);
    assert!(multi[0].iter().any(|p| p.ends_with("journal.md")));
    assert!(multi[0].iter().any(|p| p.ends_with("README.md")));
}

#[test]
fn test_readme_without_contents_is_not_a_second_index() {
    // A README.md-named file without a `contents:` property is not an index;
    // having it alongside a real index should not raise MultipleIndexes.
    let fs = make_test_fs();
    fs.write_file(
        Path::new("journal.md"),
        "---\ntitle: Journal\ncontents:\n  - README.md\n---\n",
    )
    .unwrap();
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Placeholder\npart_of: journal.md\n---\nThis is not an index.\n",
    )
    .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_file(Path::new("journal.md"))).unwrap();

    let has_multi = result
        .warnings
        .iter()
        .any(|w| matches!(w, ValidationWarning::MultipleIndexes { .. }));
    assert!(
        !has_multi,
        "README.md without `contents:` should not count as a second index, got: {:?}",
        result.warnings
    );
}

#[test]
fn test_validate_file_works_with_in_memory_fs() {
    // Regression: validate_file used to call path.canonicalize(), which
    // silently no-ops on the real FS but makes the function unusable on
    // InMemoryFileSystem (and WASM). Confirm a plain relative path works.
    let fs = make_test_fs();
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents:\n  - note.md\n---\n",
    )
    .unwrap();
    fs.write_file(
        Path::new("note.md"),
        "---\ntitle: Note\npart_of: README.md\n---\n",
    )
    .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_file(Path::new("note.md"))).unwrap();

    assert!(
        result.is_ok(),
        "validate_file should succeed for valid file, got errors: {:?}",
        result.errors
    );
    assert_eq!(result.files_checked, 1);
}

#[test]
fn test_fix_invalid_attachment_ref_legacy_binary() {
    // An index lists a raw binary in `attachments` (legacy flat format).
    // The autofix should wrap the binary in a `.md` attachment note and
    // rewrite the `attachments` entry to point at the note.
    let fs = make_test_fs();
    fs.write_file(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents:\n  - pictures.md\n---\n",
    )
    .unwrap();
    fs.write_file(
        Path::new("pictures.md"),
        "---\ntitle: Pictures\npart_of: README.md\ncontents: []\nattachments:\n  - photo.HEIC\n---\n",
    )
    .unwrap();
    fs.write_file(Path::new("photo.HEIC"), "binary content")
        .unwrap();

    let async_fs: TestFs = SyncToAsyncFs::new(fs.clone());
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();

    let warning = result
        .warnings
        .iter()
        .find(|w| matches!(w, ValidationWarning::InvalidAttachmentRef { .. }))
        .expect("expected InvalidAttachmentRef warning");
    assert!(
        warning.can_auto_fix(),
        "LegacyBinary kind must be auto-fixable"
    );

    let async_fs: TestFs = SyncToAsyncFs::new(fs.clone());
    let fixer = ValidationFixer::new(async_fs);
    let fix = block_on_test(fixer.fix_warning(warning))
        .expect("fix_warning should return a result for LegacyBinary");
    assert!(fix.success, "fix failed: {}", fix.message);

    // The wrapper note was created with both the `attachment:` property and
    // a seeded `attachment_of:` backlink pointing at the source index, so a
    // second pass through the backlink autofix is not required.
    let note = fs.read_to_string(Path::new("photo.HEIC.md")).unwrap();
    assert!(
        note.contains("attachment:"),
        "wrapper note missing attachment prop: {note}"
    );
    assert!(
        note.contains("attachment_of:"),
        "wrapper note missing attachment_of backlink: {note}"
    );

    // The source index's attachments list now points at the note, not the
    // raw binary.
    let index_content = fs.read_to_string(Path::new("pictures.md")).unwrap();
    let parsed = crate::frontmatter::parse_or_empty(&index_content).unwrap();
    let attachments = crate::frontmatter::get_string_array(&parsed.frontmatter, "attachments");
    assert_eq!(attachments.len(), 1);
    assert!(
        !attachments.iter().any(|a| a == "photo.HEIC"),
        "raw binary entry should have been replaced: {attachments:?}"
    );
    assert!(
        attachments.iter().any(|a| a.contains("photo.HEIC.md")),
        "expected attachments to reference photo.HEIC.md: {attachments:?}"
    );

    // Re-validating should no longer produce the InvalidAttachmentRef warning.
    let async_fs: TestFs = SyncToAsyncFs::new(fs);
    let validator = Validator::new(async_fs);
    let result = block_on_test(validator.validate_workspace(Path::new("README.md"), None)).unwrap();
    assert!(
        !result
            .warnings
            .iter()
            .any(|w| matches!(w, ValidationWarning::InvalidAttachmentRef { .. })),
        "InvalidAttachmentRef should be gone after fix, got: {:?}",
        result.warnings
    );
}
