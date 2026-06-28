#![allow(deprecated)]

use super::*;
use crate::fs::{FileSystem, InMemoryFileSystem, SyncToAsyncFs, block_on_test};

type TestFs = SyncToAsyncFs<InMemoryFileSystem>;

fn make_test_fs() -> TestFs {
    SyncToAsyncFs::new(InMemoryFileSystem::new())
}

#[test]
fn test_index_frontmatter_is_root() {
    let root_fm = IndexFrontmatter {
        title: Some("Root".to_string()),
        link: None,
        description: None,
        contents: Some(vec![]),
        links: None,
        link_of: None,
        part_of: None,
        audience: None,
        attachments: None,
        attachment: None,
        attachment_of: None,
        exclude: None,
        extra: std::collections::HashMap::new(),
    };
    assert!(root_fm.is_root());
    assert!(root_fm.is_index());

    let non_root_fm = IndexFrontmatter {
        title: Some("Non-root".to_string()),
        link: None,
        description: None,
        contents: Some(vec![]),
        links: None,
        link_of: None,
        part_of: Some("../parent.md".to_string()),
        audience: None,
        attachments: None,
        attachment: None,
        attachment_of: None,
        exclude: None,
        extra: std::collections::HashMap::new(),
    };
    assert!(!non_root_fm.is_root());
    assert!(non_root_fm.is_index());
}

#[test]
fn test_tree_node_formatting() {
    let tree = TreeNode {
        name: "Root".to_string(),
        description: Some("Root description".to_string()),
        path: PathBuf::from("root.md"),
        is_index: true,
        children: vec![
            TreeNode {
                name: "Child 1".to_string(),
                description: None,
                path: PathBuf::from("child1.md"),
                is_index: false,
                children: vec![],
                properties: std::collections::HashMap::new(),
                audience: Vec::new(),
            },
            TreeNode {
                name: "Child 2".to_string(),
                description: Some("Child desc".to_string()),
                path: PathBuf::from("child2.md"),
                is_index: false,
                children: vec![],
                properties: std::collections::HashMap::new(),
                audience: Vec::new(),
            },
        ],
        properties: std::collections::HashMap::new(),
        audience: Vec::new(),
    };

    let fs = make_test_fs();
    let ws = Workspace::new(fs);
    let output = ws.format_tree(&tree, "");

    assert!(output.contains("Root - Root description"));
    assert!(output.contains("Child 1"));
    assert!(output.contains("Child 2 - Child desc"));
}

#[test]
fn test_parse_index() {
    let fs = InMemoryFileSystem::new();
    fs.write(
        Path::new("test.md"),
        "---\ntitle: Test\ncontents: []\n---\n\nBody content".as_bytes(),
    )
    .unwrap();

    let async_fs = SyncToAsyncFs::new(fs);
    let ws = Workspace::new(async_fs);

    let result = block_on_test(ws.parse_index(Path::new("test.md")));
    assert!(result.is_ok());

    let index = result.unwrap();
    assert_eq!(index.frontmatter.title, Some("Test".to_string()));
    assert!(index.frontmatter.is_index());
    assert!(index.body.contains("Body content"));
}

#[test]
fn test_resolve_title_strips_inline_comment() {
    // Regression: an inline YAML comment after the title must not leak into
    // the resolved title (and thence into links referencing this file).
    let fs = InMemoryFileSystem::new();
    fs.write(
        Path::new("note.md"),
        "---\ntitle: My Title # note to self\n---\n\nBody".as_bytes(),
    )
    .unwrap();
    fs.write(
        Path::new("quoted.md"),
        "---\ntitle: \"a # b\"\n---\n\nBody".as_bytes(),
    )
    .unwrap();

    let async_fs = SyncToAsyncFs::new(fs);
    let ws = Workspace::with_link_format(async_fs, PathBuf::from(""), LinkFormat::PlainRelative);

    assert_eq!(block_on_test(ws.resolve_title("note.md")), "My Title");
    // A '#' inside a quoted scalar is part of the value, not a comment.
    assert_eq!(block_on_test(ws.resolve_title("quoted.md")), "a # b");
}

#[test]
fn test_contents_add_remove_preserves_order_and_comments() {
    // Adding/removing a child must keep the user's manual order (no
    // alphabetical re-sort) and preserve per-item comments.
    let fs = InMemoryFileSystem::new();
    fs.write(
        Path::new("index.md"),
        "---\ntitle: Idx\ncontents:\n- z.md # zulu\n- a.md # alpha\n---\nbody\n".as_bytes(),
    )
    .unwrap();
    let async_fs = SyncToAsyncFs::new(fs);
    let ws = Workspace::with_link_format(async_fs, PathBuf::from(""), LinkFormat::PlainRelative);

    // Add: appends at end, does NOT sort to [a, m, z].
    block_on_test(ws.add_to_index_contents(Path::new("index.md"), "m.md")).unwrap();
    let after_add = block_on_test(ws.fs.read_to_string(Path::new("index.md"))).unwrap();
    let order: Vec<String> = crate::frontmatter::parse(&after_add)
        .unwrap()
        .frontmatter
        .get("contents")
        .unwrap()
        .as_sequence()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(order, vec!["z.md", "a.md", "m.md"], "order not preserved");
    assert!(after_add.contains("# zulu") && after_add.contains("# alpha"));

    // Remove: keeps remaining order, preserves surviving comments.
    block_on_test(ws.remove_from_index_contents(Path::new("index.md"), "z.md")).unwrap();
    let after_rm = block_on_test(ws.fs.read_to_string(Path::new("index.md"))).unwrap();
    let order2: Vec<String> = crate::frontmatter::parse(&after_rm)
        .unwrap()
        .frontmatter
        .get("contents")
        .unwrap()
        .as_sequence()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(order2, vec!["a.md", "m.md"]);
    assert!(after_rm.contains("# alpha"), "surviving comment lost");
    assert!(!after_rm.contains("# zulu"));
}

#[test]
fn test_get_workspace_config_reads_daily_entry_folder() {
    let fs = InMemoryFileSystem::new();
    fs.write(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents: []\ndaily_entry_folder: Journal/Daily\n---\n".as_bytes(),
    )
    .unwrap();

    let async_fs = SyncToAsyncFs::new(fs);
    let ws = Workspace::new(async_fs);

    let config = block_on_test(ws.get_workspace_config(Path::new("README.md"))).unwrap();
    assert_eq!(config.daily_entry_folder.as_deref(), Some("Journal/Daily"));
}

#[test]
fn test_get_workspace_config_reads_theme_selection_fields() {
    let fs = InMemoryFileSystem::new();
    fs.write(
            Path::new("README.md"),
            "---\ntitle: Root\ncontents: []\nworkspace_config:\n  theme_mode: dark\n  theme_preset: nord\n  theme_accent_hue: 210\n---\n".as_bytes(),
        )
        .unwrap();

    let async_fs = SyncToAsyncFs::new(fs);
    let ws = Workspace::new(async_fs);

    let config = block_on_test(ws.get_workspace_config(Path::new("README.md"))).unwrap();
    assert_eq!(config.theme_mode.as_deref(), Some("dark"));
    assert_eq!(config.theme_preset.as_deref(), Some("nord"));
    assert_eq!(config.theme_accent_hue, Some(210.0));
}

#[test]
fn test_get_workspace_config_audiences_full_shape() {
    let fs = InMemoryFileSystem::new();
    fs.write(
        Path::new("README.md"),
        r#"---
title: Root
contents: []
audiences:
  - name: Public
    share_actions:
      - kind: email
        recipients:
          - friend@example.com
        subject_template: "New from me: {{title}}"
      - kind: copy_link
        label: "Share anywhere"
  - name: Family
    gates:
      - kind: link
    share_actions:
      - kind: email
        recipients:
          - mom@example.com
          - dad@example.com
  - name: Close
    gates:
      - kind: password
      - kind: link
    share_actions:
      - kind: copy_link
        label: "For the group chat"
audiences_migrated: true
---
"#
        .as_bytes(),
    )
    .unwrap();

    let async_fs = SyncToAsyncFs::new(fs);
    let ws = Workspace::new(async_fs);

    let config = block_on_test(ws.get_workspace_config(Path::new("README.md"))).unwrap();
    let publish = config.publish.clone().expect("publish parsed");
    let audiences = publish.audiences.expect("audiences parsed");
    assert_eq!(audiences.len(), 3);

    // Public: empty gates, two share actions.
    assert_eq!(audiences[0].name, "Public");
    assert!(audiences[0].gates.is_empty());
    assert_eq!(audiences[0].share_actions.len(), 2);
    match &audiences[0].share_actions[0] {
        ShareAction::Email {
            recipients,
            subject_template,
            ..
        } => {
            assert_eq!(recipients, &vec!["friend@example.com".to_string()]);
            assert_eq!(subject_template.as_deref(), Some("New from me: {{title}}"));
        }
        _ => panic!("expected email share action"),
    }

    // Family: link-only.
    assert_eq!(audiences[1].name, "Family");
    assert_eq!(audiences[1].gates, vec![Gate::Link]);

    // Close: stacked password + link gates.
    assert_eq!(audiences[2].name, "Close");
    assert_eq!(audiences[2].gates, vec![Gate::Password, Gate::Link]);

    assert_eq!(publish.audiences_migrated, Some(true));
}

#[test]
fn test_get_workspace_config_audiences_absent_is_none() {
    let fs = InMemoryFileSystem::new();
    fs.write(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents: []\n---\n".as_bytes(),
    )
    .unwrap();
    let async_fs = SyncToAsyncFs::new(fs);
    let ws = Workspace::new(async_fs);

    let config = block_on_test(ws.get_workspace_config(Path::new("README.md"))).unwrap();
    assert!(config.publish.is_none());
}

#[test]
fn test_get_workspace_config_audiences_unknown_share_kind_drops_list() {
    // Unknown variant on any single entry trips the strict serde
    // deserializer, which drops the whole list to None. This is the
    // intentional "fail loud, don't silently accept partial garbage"
    // behavior.
    let fs = InMemoryFileSystem::new();
    fs.write(
            Path::new("README.md"),
            "---\ntitle: Root\ncontents: []\naudiences:\n  - name: Family\n    share_actions:\n      - kind: discord_webhook\n---\n".as_bytes(),
        )
        .unwrap();
    let async_fs = SyncToAsyncFs::new(fs);
    let ws = Workspace::new(async_fs);

    let config = block_on_test(ws.get_workspace_config(Path::new("README.md"))).unwrap();
    assert!(config.publish.and_then(|p| p.audiences).is_none());
}

#[test]
fn test_get_workspace_config_audiences_default_gates_empty() {
    // An audience with no `gates:` key should parse with an empty Vec
    // (== public), not fail.
    let fs = InMemoryFileSystem::new();
    fs.write(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents: []\naudiences:\n  - name: Public\n---\n".as_bytes(),
    )
    .unwrap();
    let async_fs = SyncToAsyncFs::new(fs);
    let ws = Workspace::new(async_fs);

    let config = block_on_test(ws.get_workspace_config(Path::new("README.md"))).unwrap();
    let audiences = config
        .publish
        .and_then(|p| p.audiences)
        .expect("audiences parsed");
    assert_eq!(audiences.len(), 1);
    assert_eq!(audiences[0].name, "Public");
    assert!(audiences[0].gates.is_empty());
    assert!(audiences[0].share_actions.is_empty());
}

#[test]
fn test_build_tree_deduplicates_duplicate_contents_refs() {
    let fs = InMemoryFileSystem::new();
    fs.write(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents:\n  - child.md\n  - child.md\n---\n".as_bytes(),
    )
    .unwrap();
    fs.write(Path::new("child.md"), "---\ntitle: Child\n---\n".as_bytes())
        .unwrap();

    let async_fs = SyncToAsyncFs::new(fs);
    let ws = Workspace::new(async_fs);

    let tree = block_on_test(ws.build_tree(Path::new("README.md"))).unwrap();
    assert_eq!(tree.children.len(), 1);
    assert_eq!(tree.children[0].path, PathBuf::from("child.md"));
}

#[test]
fn test_is_index_file() {
    let fs = InMemoryFileSystem::new();
    fs.write(
        Path::new("index.md"),
        "---\ntitle: Index\ncontents: []\n---\n".as_bytes(),
    )
    .unwrap();
    fs.write(Path::new("leaf.md"), "---\ntitle: Leaf\n---\n".as_bytes())
        .unwrap();

    let async_fs = SyncToAsyncFs::new(fs);
    let ws = Workspace::new(async_fs);

    assert!(block_on_test(ws.is_index_file(Path::new("index.md"))));
    assert!(!block_on_test(ws.is_index_file(Path::new("leaf.md"))));
    assert!(!block_on_test(
        ws.is_index_file(Path::new("nonexistent.md"))
    ));
}

#[test]
fn test_is_root_index() {
    let fs = InMemoryFileSystem::new();
    fs.write(
        Path::new("root.md"),
        "---\ntitle: Root\ncontents: []\n---\n".as_bytes(),
    )
    .unwrap();
    fs.write(
        Path::new("child.md"),
        "---\ntitle: Child\ncontents: []\npart_of: root.md\n---\n".as_bytes(),
    )
    .unwrap();

    let async_fs = SyncToAsyncFs::new(fs);
    let ws = Workspace::new(async_fs);

    assert!(block_on_test(ws.is_root_index(Path::new("root.md"))));
    assert!(!block_on_test(ws.is_root_index(Path::new("child.md"))));
}

#[test]
fn test_find_root_index_in_dir_sync() {
    let fs = InMemoryFileSystem::new();
    fs.write(
        Path::new("root.md"),
        "---\ntitle: Root\ncontents: []\n---\n".as_bytes(),
    )
    .unwrap();
    fs.write(
        Path::new("child.md"),
        "---\ntitle: Child\ncontents: []\npart_of: root.md\n---\n".as_bytes(),
    )
    .unwrap();
    fs.write(Path::new("plain.md"), "No frontmatter here".as_bytes())
        .unwrap();

    let result = find_root_index_in_dir_sync(&fs, Path::new(".")).unwrap();
    assert_eq!(result, Some(PathBuf::from("root.md")));
}

#[test]
fn test_find_root_index_in_dir_sync_returns_none_when_no_root() {
    let fs = InMemoryFileSystem::new();
    fs.write(
        Path::new("child.md"),
        "---\ntitle: Child\ncontents: []\npart_of: root.md\n---\n".as_bytes(),
    )
    .unwrap();

    let result = find_root_index_in_dir_sync(&fs, Path::new(".")).unwrap();
    assert_eq!(result, None);
}

#[test]
fn test_rename_entry_updates_parent_contents_without_dropping_child() {
    let fs = InMemoryFileSystem::new();
    fs.write(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents:\n  - new-entry.md\n---\n".as_bytes(),
    )
    .unwrap();
    fs.write(
        Path::new("new-entry.md"),
        "---\ntitle: New Entry\npart_of: README.md\n---\n\n# New Entry\n".as_bytes(),
    )
    .unwrap();

    let async_fs = SyncToAsyncFs::new(fs);
    let ws = Workspace::new(async_fs);

    let renamed = block_on_test(ws.rename_entry(Path::new("new-entry.md"), "test.md")).unwrap();
    assert_eq!(renamed, PathBuf::from("test.md"));

    let contents =
        block_on_test(ws.get_frontmatter_property(Path::new("README.md"), "contents")).unwrap();
    let entries = match contents {
        Some(yaml::Value::Sequence(items)) => items
            .into_iter()
            .filter_map(|item| item.as_str().map(ToString::to_string))
            .collect::<Vec<_>>(),
        other => panic!("expected contents sequence, got {:?}", other),
    };

    assert!(entries.iter().any(|entry| entry.contains("test.md")));
    assert!(!entries.iter().any(|entry| entry.contains("new-entry.md")));

    let tree = block_on_test(ws.build_tree(Path::new("README.md"))).unwrap();
    assert_eq!(tree.children.len(), 1);
    assert_eq!(tree.children[0].path, PathBuf::from("test.md"));
}

#[test]
fn test_rename_entry_removes_old_from_parent_contents_with_root_path() {
    // Regression: when root_path is set and files are in subdirectories,
    // the old canonical path (e.g. "journal/old.md") was parsed as
    // Ambiguous and double-resolved relative to the index's directory,
    // producing "journal/journal/old.md" which never matched the stored
    // workspace-root link "[Old](/journal/old.md)".
    let fs = InMemoryFileSystem::new();
    let root = PathBuf::from("/ws");
    fs.create_dir_all(Path::new("/ws/journal")).unwrap();
    fs.write(
        Path::new("/ws/journal/journal.md"),
        "---\ntitle: Journal\ncontents:\n  - \"[Old Entry](/journal/old-entry.md)\"\n---\n"
            .as_bytes(),
    )
    .unwrap();
    fs.write(
        Path::new("/ws/journal/old-entry.md"),
        "---\ntitle: Old Entry\npart_of: \"[Journal](/journal/journal.md)\"\n---\n\n# Old Entry\n"
            .as_bytes(),
    )
    .unwrap();

    let async_fs = SyncToAsyncFs::new(fs);
    let ws = Workspace::with_link_format(async_fs, root, LinkFormat::MarkdownRoot);

    let renamed =
        block_on_test(ws.rename_entry(Path::new("/ws/journal/old-entry.md"), "new-entry.md"))
            .unwrap();
    assert_eq!(renamed, PathBuf::from("/ws/journal/new-entry.md"));

    let contents =
        block_on_test(ws.get_frontmatter_property(Path::new("/ws/journal/journal.md"), "contents"))
            .unwrap();
    let entries = match contents {
        Some(yaml::Value::Sequence(items)) => items
            .into_iter()
            .filter_map(|item| item.as_str().map(ToString::to_string))
            .collect::<Vec<_>>(),
        other => panic!("expected contents sequence, got {:?}", other),
    };

    assert!(
        entries.iter().any(|e| e.contains("new-entry.md")),
        "new entry should be in contents: {:?}",
        entries
    );
    assert!(
        !entries.iter().any(|e| e.contains("old-entry.md")),
        "old entry should NOT be in contents: {:?}",
        entries
    );
    assert_eq!(
        entries.len(),
        1,
        "should have exactly one entry: {:?}",
        entries
    );
}

#[test]
fn test_rename_root_index() {
    let fs = InMemoryFileSystem::new();
    fs.write(
        Path::new("README.md"),
        "---\ntitle: My Site\ncontents:\n  - child.md\n---\n".as_bytes(),
    )
    .unwrap();
    fs.write(
        Path::new("child.md"),
        "---\ntitle: Child\npart_of: README.md\n---\n".as_bytes(),
    )
    .unwrap();

    let async_fs = SyncToAsyncFs::new(fs.clone());
    let ws = Workspace::new(async_fs);

    let renamed = block_on_test(ws.rename_entry(Path::new("README.md"), "My Site.md")).unwrap();
    assert_eq!(renamed, PathBuf::from("My Site.md"));

    // File should exist at new path
    assert!(fs.try_exists(Path::new("My Site.md")).unwrap_or(false));
    assert!(!fs.try_exists(Path::new("README.md")).unwrap_or(false));

    // Child's part_of should be updated to point to the new filename
    let part_of =
        block_on_test(ws.get_frontmatter_property(Path::new("child.md"), "part_of")).unwrap();
    let part_of_str = part_of.unwrap();
    let part_of_str = part_of_str.as_str().unwrap();
    assert!(
        part_of_str.contains("My Site.md"),
        "Expected part_of to contain 'My Site.md', got '{}'",
        part_of_str
    );
}

#[test]
fn test_rename_root_index_updates_nested_child_part_of() {
    let fs = InMemoryFileSystem::new();
    let root = PathBuf::from("/ws");
    fs.create_dir_all(Path::new("/ws/Section")).unwrap();
    fs.write(
        Path::new("/ws/README.md"),
        "---\ntitle: Root\ncontents:\n  - \"[Section](/Section/section.md)\"\n---\n".as_bytes(),
    )
    .unwrap();
    fs.write(
        Path::new("/ws/Section/section.md"),
        "---\ntitle: Section\npart_of: \"[Root](/README.md)\"\n---\n".as_bytes(),
    )
    .unwrap();

    let async_fs = SyncToAsyncFs::new(fs.clone());
    let ws = Workspace::with_link_format(async_fs, root, LinkFormat::MarkdownRoot);

    let renamed = block_on_test(ws.rename_entry(Path::new("/ws/README.md"), "Home.md")).unwrap();
    assert_eq!(renamed, PathBuf::from("/ws/Home.md"));

    let part_of =
        block_on_test(ws.get_frontmatter_property(Path::new("/ws/Section/section.md"), "part_of"))
            .unwrap();
    let part_of_str = part_of.unwrap();
    let part_of_str = part_of_str.as_str().unwrap();
    assert!(
        part_of_str.contains("/Home.md"),
        "Expected part_of to contain '/Home.md', got '{}'",
        part_of_str
    );
    assert!(
        !part_of_str.contains("/README.md"),
        "Expected part_of to no longer reference README.md, got '{}'",
        part_of_str
    );
}

#[test]
fn test_rename_index_updates_nested_child_part_of() {
    let fs = InMemoryFileSystem::new();
    let root = PathBuf::from("/ws");
    fs.create_dir_all(Path::new("/ws/Section/Sub")).unwrap();
    fs.write(
        Path::new("/ws/README.md"),
        "---\ntitle: Root\ncontents:\n  - \"[Section](/Section/Section.md)\"\n---\n".as_bytes(),
    )
    .unwrap();
    fs.write(
            Path::new("/ws/Section/Section.md"),
            "---\ntitle: Section\npart_of: \"[Root](/README.md)\"\ncontents:\n  - \"[Sub](/Section/Sub/sub.md)\"\n---\n".as_bytes(),
        )
        .unwrap();
    fs.write(
        Path::new("/ws/Section/Sub/sub.md"),
        "---\ntitle: Sub\npart_of: \"[Section](/Section/Section.md)\"\n---\n".as_bytes(),
    )
    .unwrap();

    let async_fs = SyncToAsyncFs::new(fs.clone());
    let ws = Workspace::with_link_format(async_fs, root, LinkFormat::MarkdownRoot);

    let renamed =
        block_on_test(ws.rename_entry(Path::new("/ws/Section/Section.md"), "Renamed.md")).unwrap();
    assert_eq!(renamed, PathBuf::from("/ws/Renamed/Renamed.md"));

    let part_of =
        block_on_test(ws.get_frontmatter_property(Path::new("/ws/Renamed/Sub/sub.md"), "part_of"))
            .unwrap();
    let part_of_str = part_of.unwrap();
    let part_of_str = part_of_str.as_str().unwrap();
    assert!(
        part_of_str.contains("/Renamed/Renamed.md"),
        "Expected part_of to contain '/Renamed/Renamed.md', got '{}'",
        part_of_str
    );
    assert!(
        !part_of_str.contains("/Section/Section.md"),
        "Expected part_of to no longer reference '/Section/Section.md', got '{}'",
        part_of_str
    );
}

#[test]
fn test_sync_create_metadata_uses_nearest_ancestor_index() {
    let fs = InMemoryFileSystem::new();
    fs.create_dir_all(Path::new("notes/deep")).unwrap();
    fs.write(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents: []\n---\n".as_bytes(),
    )
    .unwrap();
    fs.write(
        Path::new("notes/deep/new.md"),
        "---\ntitle: New Note\n---\n\n# New Note\n".as_bytes(),
    )
    .unwrap();

    let async_fs = SyncToAsyncFs::new(fs);
    let ws = Workspace::new(async_fs);

    block_on_test(ws.sync_create_metadata(Path::new("notes/deep/new.md"))).unwrap();

    let contents =
        block_on_test(ws.get_frontmatter_property(Path::new("README.md"), "contents")).unwrap();
    let contents = match contents {
        Some(yaml::Value::Sequence(items)) => items
            .into_iter()
            .filter_map(|item| item.as_str().map(ToString::to_string))
            .collect::<Vec<_>>(),
        other => panic!("expected contents sequence, got {:?}", other),
    };
    assert!(
        contents
            .iter()
            .any(|entry| entry.contains("notes/deep/new.md")),
        "expected README contents to include moved file, got {:?}",
        contents
    );

    let part_of =
        block_on_test(ws.get_frontmatter_property(Path::new("notes/deep/new.md"), "part_of"))
            .unwrap();
    let part_of = part_of.and_then(|v| v.as_str().map(ToString::to_string));
    assert_eq!(part_of.as_deref(), Some("../../README.md"));
}

#[test]
fn test_sync_move_metadata_updates_hierarchy_with_nearest_ancestor_index() {
    let fs = InMemoryFileSystem::new();
    fs.create_dir_all(Path::new("nested/deep")).unwrap();
    fs.write(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents:\n  - old.md\n---\n".as_bytes(),
    )
    .unwrap();
    fs.write(
        Path::new("old.md"),
        "---\ntitle: Old\npart_of: README.md\n---\n\n# Old\n".as_bytes(),
    )
    .unwrap();

    // Simulate external move (Obsidian already moved the file).
    fs.rename(Path::new("old.md"), Path::new("nested/deep/new.md"))
        .unwrap();

    let async_fs = SyncToAsyncFs::new(fs);
    let ws = Workspace::new(async_fs);

    block_on_test(ws.sync_move_metadata(Path::new("old.md"), Path::new("nested/deep/new.md")))
        .unwrap();

    let contents =
        block_on_test(ws.get_frontmatter_property(Path::new("README.md"), "contents")).unwrap();
    let contents = match contents {
        Some(yaml::Value::Sequence(items)) => items
            .into_iter()
            .filter_map(|item| item.as_str().map(ToString::to_string))
            .collect::<Vec<_>>(),
        other => panic!("expected contents sequence, got {:?}", other),
    };

    assert!(
        contents
            .iter()
            .any(|entry| entry.contains("nested/deep/new.md")),
        "expected README contents to include new path, got {:?}",
        contents
    );
    assert!(
        !contents.iter().any(|entry| entry.contains("old.md")),
        "expected README contents to remove old path, got {:?}",
        contents
    );

    let part_of =
        block_on_test(ws.get_frontmatter_property(Path::new("nested/deep/new.md"), "part_of"))
            .unwrap();
    let part_of = part_of.and_then(|v| v.as_str().map(ToString::to_string));
    assert_eq!(part_of.as_deref(), Some("../../README.md"));
}

#[test]
fn test_sync_move_metadata_clears_part_of_when_destination_has_no_parent_index() {
    let fs = InMemoryFileSystem::new();
    fs.create_dir_all(Path::new("section")).unwrap();
    fs.create_dir_all(Path::new("outside")).unwrap();
    fs.write(
        Path::new("section/index.md"),
        "---\ntitle: Section\ncontents:\n  - child.md\n---\n".as_bytes(),
    )
    .unwrap();
    fs.write(
        Path::new("section/child.md"),
        "---\ntitle: Child\npart_of: index.md\n---\n\n# Child\n".as_bytes(),
    )
    .unwrap();

    // Simulate external move into a location with no ancestor index.
    fs.rename(Path::new("section/child.md"), Path::new("outside/new.md"))
        .unwrap();

    let async_fs = SyncToAsyncFs::new(fs);
    let ws = Workspace::new(async_fs);

    block_on_test(
        ws.sync_move_metadata(Path::new("section/child.md"), Path::new("outside/new.md")),
    )
    .unwrap();

    let contents =
        block_on_test(ws.get_frontmatter_property(Path::new("section/index.md"), "contents"))
            .unwrap();
    let contents = match contents {
        Some(yaml::Value::Sequence(items)) => items
            .into_iter()
            .filter_map(|item| item.as_str().map(ToString::to_string))
            .collect::<Vec<_>>(),
        other => panic!("expected contents sequence, got {:?}", other),
    };
    assert!(
        !contents.iter().any(|entry| entry.contains("child.md")),
        "expected section index to drop old child reference, got {:?}",
        contents
    );

    let part_of =
        block_on_test(ws.get_frontmatter_property(Path::new("outside/new.md"), "part_of")).unwrap();
    assert!(
        part_of.is_none(),
        "expected part_of to be removed when destination has no parent index"
    );
}

#[test]
fn test_sync_delete_metadata_uses_nearest_ancestor_index() {
    let fs = InMemoryFileSystem::new();
    fs.create_dir_all(Path::new("nested/deep")).unwrap();
    fs.write(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents:\n  - nested/deep/victim.md\n---\n".as_bytes(),
    )
    .unwrap();
    fs.write(
        Path::new("nested/deep/victim.md"),
        "---\ntitle: Victim\npart_of: ../../README.md\n---\n\n# Victim\n".as_bytes(),
    )
    .unwrap();

    // Simulate external delete (Obsidian already deleted the file).
    fs.remove_file(Path::new("nested/deep/victim.md")).unwrap();

    let async_fs = SyncToAsyncFs::new(fs);
    let ws = Workspace::new(async_fs);

    block_on_test(ws.sync_delete_metadata(Path::new("nested/deep/victim.md"))).unwrap();

    let contents =
        block_on_test(ws.get_frontmatter_property(Path::new("README.md"), "contents")).unwrap();
    let contents = match contents {
        Some(yaml::Value::Sequence(items)) => items
            .into_iter()
            .filter_map(|item| item.as_str().map(ToString::to_string))
            .collect::<Vec<_>>(),
        other => panic!("expected contents sequence, got {:?}", other),
    };
    assert!(
        !contents
            .iter()
            .any(|entry| entry.contains("nested/deep/victim.md")),
        "expected README contents to remove deleted file, got {:?}",
        contents
    );
}

// -------------------------------------------------------------------------
// attach_entry_to_parent
// -------------------------------------------------------------------------

fn get_contents_strings(
    ws: &Workspace<SyncToAsyncFs<InMemoryFileSystem>>,
    index: &str,
) -> Vec<String> {
    let val = block_on_test(ws.get_frontmatter_property(Path::new(index), "contents")).unwrap();
    match val {
        Some(yaml::Value::Sequence(items)) => items
            .into_iter()
            .filter_map(|v| v.as_str().map(ToString::to_string))
            .collect(),
        _ => vec![],
    }
}

#[test]
fn test_attach_entry_to_parent_adds_to_new_parent_contents() {
    let fs = InMemoryFileSystem::new();
    fs.create_dir_all(Path::new("section")).unwrap();
    fs.write(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents: []\n---\n".as_bytes(),
    )
    .unwrap();
    fs.write(
        Path::new("section/index.md"),
        "---\ntitle: Section\ncontents: []\npart_of: ../README.md\n---\n".as_bytes(),
    )
    .unwrap();
    fs.write(
        Path::new("section/note.md"),
        "---\ntitle: Note\n---\n".as_bytes(),
    )
    .unwrap();

    let async_fs = SyncToAsyncFs::new(fs);
    let ws = Workspace::new(async_fs);

    block_on_test(
        ws.attach_entry_to_parent(Path::new("section/note.md"), Path::new("section/index.md")),
    )
    .unwrap();

    let contents = get_contents_strings(&ws, "section/index.md");
    assert!(
        contents.iter().any(|e| e.contains("note.md")),
        "expected section/index.md contents to include note.md, got {:?}",
        contents
    );

    let part_of =
        block_on_test(ws.get_frontmatter_property(Path::new("section/note.md"), "part_of"))
            .unwrap();
    assert!(part_of.is_some(), "expected part_of to be set on note.md");
}

#[test]
fn test_attach_entry_to_parent_removes_from_old_parent_contents() {
    // note.md starts as a child of README.md (root). We re-attach it to
    // section/index.md (same directory as note.md). The root's contents
    // should no longer reference note.md after the call.
    let fs = InMemoryFileSystem::new();
    fs.create_dir_all(Path::new("section")).unwrap();
    fs.write(
        Path::new("section/README.md"),
        "---\ntitle: Root\ncontents:\n  - note.md\n---\n".as_bytes(),
    )
    .unwrap();
    fs.write(
        Path::new("section/index.md"),
        "---\ntitle: Section\ncontents: []\npart_of: README.md\n---\n".as_bytes(),
    )
    .unwrap();
    fs.write(
        Path::new("section/note.md"),
        "---\ntitle: Note\npart_of: README.md\n---\n".as_bytes(),
    )
    .unwrap();

    let async_fs = SyncToAsyncFs::new(fs);
    let ws = Workspace::new(async_fs);

    block_on_test(
        ws.attach_entry_to_parent(Path::new("section/note.md"), Path::new("section/index.md")),
    )
    .unwrap();

    // note.md should now be in section/index.md's contents
    let section_contents = get_contents_strings(&ws, "section/index.md");
    assert!(
        section_contents.iter().any(|e| e.contains("note.md")),
        "expected section/index.md contents to include note.md, got {:?}",
        section_contents
    );

    // note.md should no longer appear in old parent's contents
    let root_contents = get_contents_strings(&ws, "section/README.md");
    assert!(
        !root_contents.iter().any(|e| e.contains("note.md")),
        "expected README.md contents to no longer include note.md, got {:?}",
        root_contents
    );
}

// -------------------------------------------------------------------------
// attach_and_move_entry_to_parent
// -------------------------------------------------------------------------

#[test]
fn test_attach_and_move_entry_to_parent_moves_file_and_updates_hierarchy() {
    // entry.md lives under root and should be moved into section/.
    let fs = InMemoryFileSystem::new();
    fs.create_dir_all(Path::new("section")).unwrap();
    fs.write(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents:\n  - entry.md\n---\n".as_bytes(),
    )
    .unwrap();
    fs.write(
        Path::new("section/index.md"),
        "---\ntitle: Section\ncontents: []\npart_of: ../README.md\n---\n".as_bytes(),
    )
    .unwrap();
    fs.write(
        Path::new("entry.md"),
        "---\ntitle: Entry\npart_of: README.md\n---\n".as_bytes(),
    )
    .unwrap();

    let async_fs = SyncToAsyncFs::new(fs);
    let ws = Workspace::new(async_fs);

    let new_path = block_on_test(
        ws.attach_and_move_entry_to_parent(Path::new("entry.md"), Path::new("section/index.md")),
    )
    .unwrap();

    assert_eq!(new_path, PathBuf::from("section/entry.md"));

    // File should exist at new location
    assert!(block_on_test(async {
        ws.fs
            .try_exists(Path::new("section/entry.md"))
            .await
            .unwrap_or(false)
    }));
    assert!(!block_on_test(async {
        ws.fs
            .try_exists(Path::new("entry.md"))
            .await
            .unwrap_or(false)
    }));

    // New parent should contain the entry
    let section_contents = get_contents_strings(&ws, "section/index.md");
    assert!(
        section_contents.iter().any(|e| e.contains("entry.md")),
        "expected section/index.md to contain entry.md, got {:?}",
        section_contents
    );

    // Old parent should no longer reference it
    let root_contents = get_contents_strings(&ws, "README.md");
    assert!(
        !root_contents.iter().any(|e| e.contains("entry.md")),
        "expected README.md to no longer reference entry.md, got {:?}",
        root_contents
    );
}

#[test]
fn test_attach_and_move_entry_to_parent_converts_leaf_target_to_index() {
    // Moving entry.md into leaf.md (a non-index) should convert leaf.md into
    // section/leaf/leaf.md (index) and move entry.md to section/leaf/entry.md.
    let fs = InMemoryFileSystem::new();
    fs.create_dir_all(Path::new("section")).unwrap();
    fs.write(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents:\n  - entry.md\n  - section/leaf.md\n---\n".as_bytes(),
    )
    .unwrap();
    fs.write(
        Path::new("entry.md"),
        "---\ntitle: Entry\npart_of: README.md\n---\n".as_bytes(),
    )
    .unwrap();
    fs.write(
        Path::new("section/leaf.md"),
        "---\ntitle: Leaf\npart_of: ../README.md\n---\n".as_bytes(),
    )
    .unwrap();

    let async_fs = SyncToAsyncFs::new(fs);
    let ws = Workspace::new(async_fs);

    let new_path = block_on_test(
        ws.attach_and_move_entry_to_parent(Path::new("entry.md"), Path::new("section/leaf.md")),
    )
    .unwrap();

    // convert_to_index moves section/leaf.md → section/leaf/leaf.md
    // and entry.md → section/leaf/entry.md
    assert_eq!(new_path, PathBuf::from("section/leaf/entry.md"));

    assert!(block_on_test(
        ws.is_index_file(Path::new("section/leaf/leaf.md"))
    ));
    assert!(block_on_test(async {
        ws.fs
            .try_exists(Path::new("section/leaf/entry.md"))
            .await
            .unwrap_or(false)
    }));
    assert!(!block_on_test(async {
        ws.fs
            .try_exists(Path::new("entry.md"))
            .await
            .unwrap_or(false)
    }));

    // Old parent should no longer reference entry.md at the root level
    let root_contents = get_contents_strings(&ws, "README.md");
    assert!(
        !root_contents.iter().any(|e| e == "entry.md"),
        "expected README.md to no longer reference entry.md, got {:?}",
        root_contents
    );
}

#[test]
fn test_attach_and_move_entry_to_parent_moves_folder_and_rewrites_nested_part_of() {
    // Projects/ is a folder (represented by Projects/projects.md) with a
    // nested sub-index (Projects/sub/sub.md) and a grandchild leaf
    // (Projects/sub/task.md). Every file uses markdown_root-format
    // part_of values, so absolute paths get stale after the move unless
    // we rewrite them. We drag Projects into Archive/ and expect the
    // whole tree to move AND every part_of to point at the new location.
    let fs = InMemoryFileSystem::new();
    fs.create_dir_all(Path::new("Projects/sub")).unwrap();
    fs.create_dir_all(Path::new("Archive")).unwrap();
    fs.write(
            Path::new("README.md"),
            "---\ntitle: Root\ncontents:\n  - \"[Projects](/Projects/projects.md)\"\n  - \"[Archive](/Archive/archive.md)\"\n---\n".as_bytes(),
        )
        .unwrap();
    fs.write(
        Path::new("Archive/archive.md"),
        "---\ntitle: Archive\ncontents: []\npart_of: \"[Root](/README.md)\"\n---\n".as_bytes(),
    )
    .unwrap();
    fs.write(
            Path::new("Projects/projects.md"),
            "---\ntitle: Projects\ncontents:\n  - \"[Sub](/Projects/sub/sub.md)\"\npart_of: \"[Root](/README.md)\"\n---\n".as_bytes(),
        )
        .unwrap();
    fs.write(
            Path::new("Projects/sub/sub.md"),
            "---\ntitle: Sub\ncontents:\n  - \"[Task](/Projects/sub/task.md)\"\npart_of: \"[Projects](/Projects/projects.md)\"\n---\n".as_bytes(),
        )
        .unwrap();
    fs.write(
        Path::new("Projects/sub/task.md"),
        "---\ntitle: Task\npart_of: \"[Sub](/Projects/sub/sub.md)\"\n---\n".as_bytes(),
    )
    .unwrap();

    let async_fs = SyncToAsyncFs::new(fs);
    let ws = Workspace::with_link_format(async_fs, PathBuf::from(""), LinkFormat::MarkdownRoot);

    let new_path = block_on_test(ws.attach_and_move_entry_to_parent(
        Path::new("Projects/projects.md"),
        Path::new("Archive/archive.md"),
    ))
    .unwrap();

    assert_eq!(new_path, PathBuf::from("Archive/Projects/projects.md"));

    // Folder contents moved.
    assert!(block_on_test(async {
        ws.fs
            .try_exists(Path::new("Archive/Projects/projects.md"))
            .await
            .unwrap_or(false)
    }));
    assert!(block_on_test(async {
        ws.fs
            .try_exists(Path::new("Archive/Projects/sub/sub.md"))
            .await
            .unwrap_or(false)
    }));
    assert!(block_on_test(async {
        ws.fs
            .try_exists(Path::new("Archive/Projects/sub/task.md"))
            .await
            .unwrap_or(false)
    }));
    assert!(!block_on_test(async {
        ws.fs
            .try_exists(Path::new("Projects/projects.md"))
            .await
            .unwrap_or(false)
    }));
    assert!(!block_on_test(async {
        ws.fs
            .try_exists(Path::new("Projects/sub/sub.md"))
            .await
            .unwrap_or(false)
    }));

    // Moved index's own part_of now points to the new parent.
    let projects_part_of = block_on_test(
        ws.get_frontmatter_property(Path::new("Archive/Projects/projects.md"), "part_of"),
    )
    .unwrap();
    let projects_part_of = match projects_part_of {
        Some(yaml::Value::String(s)) => s,
        other => panic!("expected projects.md part_of string, got {:?}", other),
    };
    assert!(
        projects_part_of.contains("/Archive/archive.md"),
        "projects.md part_of should point at new parent, got: {projects_part_of}"
    );

    // Nested sub-index's part_of should now reference the moved projects.md.
    let sub_part_of = block_on_test(
        ws.get_frontmatter_property(Path::new("Archive/Projects/sub/sub.md"), "part_of"),
    )
    .unwrap();
    let sub_part_of = match sub_part_of {
        Some(yaml::Value::String(s)) => s,
        other => panic!("expected sub.md part_of string, got {:?}", other),
    };
    assert!(
        sub_part_of.contains("/Archive/Projects/projects.md"),
        "sub.md part_of should point at new projects.md, got: {sub_part_of}"
    );
    assert!(
        !sub_part_of.contains("/Projects/projects.md")
            || sub_part_of.contains("/Archive/Projects/projects.md"),
        "sub.md part_of should not point at old path, got: {sub_part_of}"
    );

    // Grandchild task.md's part_of should now reference the moved sub.md
    // — this is the recursion case the bug was about.
    let task_part_of = block_on_test(
        ws.get_frontmatter_property(Path::new("Archive/Projects/sub/task.md"), "part_of"),
    )
    .unwrap();
    let task_part_of = match task_part_of {
        Some(yaml::Value::String(s)) => s,
        other => panic!("expected task.md part_of string, got {:?}", other),
    };
    assert!(
        task_part_of.contains("/Archive/Projects/sub/sub.md"),
        "task.md part_of should point at new sub.md, got: {task_part_of}"
    );
}

#[test]
fn test_collect_workspace_file_set_includes_reachable_markdown_and_attachments() {
    let fs = InMemoryFileSystem::new();
    fs.write(
        Path::new("workspace/index.md"),
        "---\ntitle: Root\ncontents:\n  - Notes/day.md\nattachments:\n  - assets/root.png\n---\n"
            .as_bytes(),
    )
    .unwrap();
    fs.write(
            Path::new("workspace/Notes/day.md"),
            "---\ntitle: Day\npart_of: /index.md\nattachments:\n  - ./_attachments/day.jpg\n  - /shared/manual.pdf\n---\n".as_bytes(),
        )
        .unwrap();
    fs.write(Path::new("workspace/assets/root.png"), "root".as_bytes())
        .unwrap();
    fs.write(
        Path::new("workspace/Notes/_attachments/day.jpg"),
        "day".as_bytes(),
    )
    .unwrap();
    fs.write(
        Path::new("workspace/shared/manual.pdf"),
        "manual".as_bytes(),
    )
    .unwrap();
    fs.write(
        Path::new("workspace/node_modules/nope.js"),
        "ignored".as_bytes(),
    )
    .unwrap();

    let async_fs = SyncToAsyncFs::new(fs);
    let ws = Workspace::new(async_fs);

    let file_set =
        block_on_test(ws.collect_workspace_file_set(Path::new("workspace/index.md"))).unwrap();

    assert_eq!(
        file_set,
        vec![
            "Notes/_attachments/day.jpg".to_string(),
            "Notes/day.md".to_string(),
            "assets/root.png".to_string(),
            "index.md".to_string(),
            "shared/manual.pdf".to_string(),
        ]
    );
}

#[test]
fn test_build_filesystem_tree_inherits_nested_index_excludes() {
    let fs = InMemoryFileSystem::new();
    fs.create_dir_all(Path::new("workspace/scripts")).unwrap();
    fs.write(
        Path::new("workspace/Diaryx.md"),
        "---\ntitle: Root\ncontents: []\nexclude:\n  - \"**/target\"\n---\n".as_bytes(),
    )
    .unwrap();
    fs.write(
        Path::new("workspace/scripts/scripts.md"),
        "---\ntitle: Scripts\ncontents: []\nexclude:\n  - \"*.sh\"\n---\n".as_bytes(),
    )
    .unwrap();
    fs.write(Path::new("workspace/scripts/keep.txt"), "keep".as_bytes())
        .unwrap();
    fs.write(Path::new("workspace/scripts/run.sh"), "echo hi".as_bytes())
        .unwrap();

    let async_fs = SyncToAsyncFs::new(fs);
    let ws = Workspace::new(async_fs);

    let tree =
        block_on_test(ws.build_filesystem_tree_with_depth(Path::new("workspace"), false, None))
            .unwrap();

    let scripts = tree
        .children
        .iter()
        .find(|child| child.path == Path::new("workspace/scripts/scripts.md"))
        .expect("scripts directory should be present");

    let child_names: Vec<_> = scripts
        .children
        .iter()
        .map(|child| child.name.as_str())
        .collect();
    assert!(
        child_names.contains(&"keep.txt"),
        "expected keep.txt to remain visible, got {:?}",
        child_names
    );
    assert!(
        !child_names.contains(&"run.sh"),
        "expected nested index exclude to hide run.sh, got {:?}",
        child_names
    );
}

#[test]
fn test_build_filesystem_tree_prunes_builtin_skip_directories() {
    let fs = InMemoryFileSystem::new();
    fs.write(
        Path::new("workspace/Diaryx.md"),
        "---\ntitle: Root\ncontents: []\n---\n".as_bytes(),
    )
    .unwrap();
    fs.write(Path::new("workspace/visible.txt"), "ok".as_bytes())
        .unwrap();
    fs.write(
        Path::new("workspace/target/debug/app.bin"),
        "bin".as_bytes(),
    )
    .unwrap();
    fs.write(
        Path::new("workspace/node_modules/pkg/index.js"),
        "js".as_bytes(),
    )
    .unwrap();

    let async_fs = SyncToAsyncFs::new(fs);
    let ws = Workspace::new(async_fs);

    let tree =
        block_on_test(ws.build_filesystem_tree_with_depth(Path::new("workspace"), false, None))
            .unwrap();

    let child_paths: Vec<_> = tree
        .children
        .iter()
        .map(|child| child.path.to_string_lossy().to_string())
        .collect();
    assert!(
        child_paths.contains(&"workspace/visible.txt".to_string()),
        "expected visible file in tree, got {:?}",
        child_paths
    );
    assert!(
        !child_paths.iter().any(|path| path.contains("target")),
        "expected target to be pruned, got {:?}",
        child_paths
    );
    assert!(
        !child_paths.iter().any(|path| path.contains("node_modules")),
        "expected node_modules to be pruned, got {:?}",
        child_paths
    );
}

#[test]
fn test_migrate_flat_config_to_file() {
    let fs = InMemoryFileSystem::new();
    fs.write(
            Path::new("README.md"),
            "---\ntitle: Root\ndescription: D\ncontents: []\nfilename_style: kebab_case\ndefault_audience: family\n---\n\n# Root\n".as_bytes(),
        )
        .unwrap();
    let ws = Workspace::new(SyncToAsyncFs::new(fs));

    let migrated =
        block_on_test(ws.migrate_workspace_config_to_file(Path::new("README.md"))).unwrap();
    assert!(migrated, "expected a migration to happen");

    // Settings file created with the migrated fields.
    let cfg = block_on_test(ws.fs.read_to_string(Path::new("Meta/Config.md"))).unwrap();
    assert!(cfg.contains("filename_style: kebab_case"), "cfg: {cfg}");
    assert!(cfg.contains("default_audience: family"), "cfg: {cfg}");

    // Root index now links to the settings file and dropped the flat fields,
    // while its content body is preserved.
    let root = block_on_test(ws.fs.read_to_string(Path::new("README.md"))).unwrap();
    assert!(root.contains("workspace_config:"), "root: {root}");
    assert!(root.contains("Meta/Config.md"), "root: {root}");
    assert!(!root.contains("filename_style"), "root: {root}");
    assert!(!root.contains("default_audience"), "root: {root}");
    assert!(root.contains("# Root"), "body lost: {root}");

    // Effective config is unchanged after migration.
    let config = block_on_test(ws.get_workspace_config(Path::new("README.md"))).unwrap();
    assert_eq!(config.filename_style, FilenameStyle::KebabCase);
    assert_eq!(config.default_audience.as_deref(), Some("family"));

    // Idempotent: a second run is a no-op.
    let again = block_on_test(ws.migrate_workspace_config_to_file(Path::new("README.md"))).unwrap();
    assert!(!again, "second migration should be a no-op");
}

#[test]
fn test_migrate_nested_config_to_file() {
    let fs = InMemoryFileSystem::new();
    fs.write(
            Path::new("README.md"),
            "---\ntitle: Root\ncontents: []\nworkspace_config:\n  theme_mode: dark\n  theme_preset: nord\n---\n".as_bytes(),
        )
        .unwrap();
    let ws = Workspace::new(SyncToAsyncFs::new(fs));

    let migrated =
        block_on_test(ws.migrate_workspace_config_to_file(Path::new("README.md"))).unwrap();
    assert!(migrated);

    let cfg = block_on_test(ws.fs.read_to_string(Path::new("Meta/Config.md"))).unwrap();
    assert!(cfg.contains("theme_mode: dark"), "cfg: {cfg}");
    assert!(cfg.contains("theme_preset: nord"), "cfg: {cfg}");

    let config = block_on_test(ws.get_workspace_config(Path::new("README.md"))).unwrap();
    assert_eq!(config.theme_mode.as_deref(), Some("dark"));
    assert_eq!(config.theme_preset.as_deref(), Some("nord"));
}

#[test]
fn test_migrate_already_linked_is_noop() {
    let fs = InMemoryFileSystem::new();
    fs.write(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents: []\nworkspace_config: \"[Config](Meta/Config.md)\"\n---\n"
            .as_bytes(),
    )
    .unwrap();
    fs.write(
        Path::new("Meta/Config.md"),
        "---\ntitle: Workspace Settings\ntheme_mode: light\n---\n".as_bytes(),
    )
    .unwrap();
    let ws = Workspace::new(SyncToAsyncFs::new(fs));

    let migrated =
        block_on_test(ws.migrate_workspace_config_to_file(Path::new("README.md"))).unwrap();
    assert!(!migrated, "already-linked workspace should not migrate");
}

#[test]
fn test_migrate_empty_workspace_is_noop() {
    let fs = InMemoryFileSystem::new();
    fs.write(
        Path::new("README.md"),
        "---\ntitle: Root\ndescription: D\ncontents: []\n---\n\n# Root\n".as_bytes(),
    )
    .unwrap();
    let ws = Workspace::new(SyncToAsyncFs::new(fs));

    let migrated =
        block_on_test(ws.migrate_workspace_config_to_file(Path::new("README.md"))).unwrap();
    assert!(!migrated, "a workspace with no config should not migrate");

    // No stray settings file should be created.
    assert!(
        block_on_test(ws.fs.read_to_string(Path::new("Meta/Config.md"))).is_err(),
        "no settings file should be created for an empty workspace"
    );
}

#[test]
fn test_migrate_merges_into_existing_config_file() {
    // A pre-existing settings file must not be clobbered: existing keys win,
    // and migrated keys only fill gaps.
    let fs = InMemoryFileSystem::new();
    fs.write(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents: []\ntheme_mode: dark\ndefault_audience: family\n---\n"
            .as_bytes(),
    )
    .unwrap();
    fs.write(
        Path::new("Meta/Config.md"),
        "---\ntitle: Workspace Settings\ntheme_mode: light\n---\n".as_bytes(),
    )
    .unwrap();
    let ws = Workspace::new(SyncToAsyncFs::new(fs));

    let migrated =
        block_on_test(ws.migrate_workspace_config_to_file(Path::new("README.md"))).unwrap();
    assert!(migrated);

    let cfg = block_on_test(ws.fs.read_to_string(Path::new("Meta/Config.md"))).unwrap();
    // Existing value preserved (not overwritten by the migrated dark).
    assert!(cfg.contains("theme_mode: light"), "cfg: {cfg}");
    assert!(!cfg.contains("theme_mode: dark"), "cfg: {cfg}");
    // New key filled in from the root index.
    assert!(cfg.contains("default_audience: family"), "cfg: {cfg}");

    let config = block_on_test(ws.get_workspace_config(Path::new("README.md"))).unwrap();
    assert_eq!(config.theme_mode.as_deref(), Some("light"));
    assert_eq!(config.default_audience.as_deref(), Some("family"));
}

#[test]
fn test_set_workspace_config_establishes_file_on_empty_workspace() {
    let fs = InMemoryFileSystem::new();
    fs.write(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents: []\n---\n".as_bytes(),
    )
    .unwrap();
    let ws = Workspace::new(SyncToAsyncFs::new(fs));

    block_on_test(ws.set_workspace_config_field(Path::new("README.md"), "theme_mode", "dark"))
        .unwrap();

    // Field landed in the linked settings file, not the root index.
    let root = block_on_test(ws.fs.read_to_string(Path::new("README.md"))).unwrap();
    assert!(root.contains("workspace_config:"), "root: {root}");
    assert!(!root.contains("theme_mode"), "root: {root}");
    let cfg = block_on_test(ws.fs.read_to_string(Path::new("Meta/Config.md"))).unwrap();
    assert!(cfg.contains("theme_mode: dark"), "cfg: {cfg}");

    // A subsequent write follows the existing link rather than creating a new file.
    block_on_test(ws.set_workspace_config_field(Path::new("README.md"), "theme_preset", "nord"))
        .unwrap();
    let config = block_on_test(ws.get_workspace_config(Path::new("README.md"))).unwrap();
    assert_eq!(config.theme_mode.as_deref(), Some("dark"));
    assert_eq!(config.theme_preset.as_deref(), Some("nord"));
}

#[test]
fn test_migrate_moves_audiences_to_file() {
    let fs = InMemoryFileSystem::new();
    fs.write(
        Path::new("README.md"),
        r#"---
title: Root
contents: []
audiences:
  - name: Public
    share_actions:
      - kind: copy_link
        label: "Share anywhere"
  - name: Family
    gates:
      - kind: link
audiences_migrated: true
---

# Root
"#
        .as_bytes(),
    )
    .unwrap();
    let ws = Workspace::new(SyncToAsyncFs::new(fs));

    let migrated =
        block_on_test(ws.migrate_workspace_config_to_file(Path::new("README.md"))).unwrap();
    assert!(migrated);

    // Audiences live in the settings file now, not the root index.
    let cfg = block_on_test(ws.fs.read_to_string(Path::new("Meta/Config.md"))).unwrap();
    assert!(cfg.contains("audiences:"), "cfg: {cfg}");
    assert!(
        cfg.contains("Public") && cfg.contains("Family"),
        "cfg: {cfg}"
    );
    let root = block_on_test(ws.fs.read_to_string(Path::new("README.md"))).unwrap();
    assert!(
        !root.contains("audiences"),
        "root still has audiences: {root}"
    );

    // The full audiences shape still parses through get_workspace_config
    // (folded under `publish` via back-compat read of top-level `audiences`).
    let config = block_on_test(ws.get_workspace_config(Path::new("README.md"))).unwrap();
    let publish = config
        .publish
        .clone()
        .expect("publish parsed after migration");
    let audiences = publish.audiences.expect("audiences parsed after migration");
    assert_eq!(audiences.len(), 2);
    assert_eq!(audiences[0].name, "Public");
    assert_eq!(audiences[1].name, "Family");
    assert_eq!(publish.audiences_migrated, Some(true));
}

#[test]
fn test_migrate_publish_config_relocates_legacy_into_publish_section() {
    let fs = InMemoryFileSystem::new();
    fs.write(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents: []\nworkspace_config: '[Config](/Meta/Config.md)'\n---\n"
            .as_bytes(),
    )
    .unwrap();
    // Settings file with the legacy locations: top-level audiences +
    // audiences_migrated, and the former plugins."diaryx.publish".config blob.
    fs.write(
        Path::new("Meta/Config.md"),
        "---\ntitle: Workspace Settings\n\
default_audience: family\n\
audiences:\n  - name: Public\n  - name: Family\n    gates:\n      - kind: link\n\
audiences_migrated: true\n\
plugins:\n  diaryx.publish:\n    config:\n      namespace_id: ns-abc\n      subdomain: my-site\n\
---\n"
            .as_bytes(),
    )
    .unwrap();
    let ws = Workspace::new(SyncToAsyncFs::new(fs));

    let migrated = block_on_test(ws.migrate_publish_config(Path::new("README.md"))).unwrap();
    assert!(migrated);

    // The settings file now has a `publish:` section; the legacy top-level
    // keys and the plugin entry are gone.
    let cfg = block_on_test(ws.fs.read_to_string(Path::new("Meta/Config.md"))).unwrap();
    assert!(cfg.contains("publish:"), "cfg: {cfg}");
    assert!(
        !cfg.contains("diaryx.publish"),
        "plugin entry remains: {cfg}"
    );
    assert!(
        !cfg.contains("\naudiences_migrated:"),
        "top-level audiences_migrated remains: {cfg}"
    );

    // get_workspace_config exposes everything under `publish`, and
    // default_audience stays top-level.
    let config = block_on_test(ws.get_workspace_config(Path::new("README.md"))).unwrap();
    assert_eq!(config.default_audience.as_deref(), Some("family"));
    let publish = config.publish.expect("publish section");
    assert_eq!(publish.namespace_id.as_deref(), Some("ns-abc"));
    assert_eq!(publish.subdomain.as_deref(), Some("my-site"));
    assert_eq!(publish.audiences_migrated, Some(true));
    let audiences = publish.audiences.expect("audiences relocated");
    assert_eq!(audiences.len(), 2);
    assert_eq!(audiences[0].name, "Public");
    assert_eq!(audiences[1].name, "Family");

    // Idempotent: a second run finds nothing to relocate.
    let again = block_on_test(ws.migrate_publish_config(Path::new("README.md"))).unwrap();
    assert!(!again, "migration should be idempotent");
}

#[test]
fn test_migrate_honors_markdown_root_link_format() {
    let fs = InMemoryFileSystem::new();
    fs.write(
            Path::new("README.md"),
            "---\ntitle: Root\ncontents: []\nlink_format: markdown_root\ndefault_audience: family\n---\n".as_bytes(),
        )
        .unwrap();
    let ws = Workspace::new(SyncToAsyncFs::new(fs));

    block_on_test(ws.migrate_workspace_config_to_file(Path::new("README.md"))).unwrap();

    // The link is written in the workspace's markdown_root format (the
    // leading slash distinguishes it from a relative link)...
    let root = block_on_test(ws.fs.read_to_string(Path::new("README.md"))).unwrap();
    assert!(
        root.contains("[Config](/Meta/Config.md)"),
        "root link not in markdown_root format: {root}"
    );

    // ...and a root-relative link still round-trips through config resolution.
    let config = block_on_test(ws.get_workspace_config(Path::new("README.md"))).unwrap();
    assert_eq!(config.link_format, LinkFormat::MarkdownRoot);
    assert_eq!(config.default_audience.as_deref(), Some("family"));
}

#[test]
fn test_set_workspace_config_follows_existing_nondefault_link() {
    // When the root index links to a settings file at a non-default path,
    // writes follow the link rather than the hardcoded default.
    let fs = InMemoryFileSystem::new();
    fs.write(
            Path::new("README.md"),
            "---\ntitle: Root\ncontents: []\nworkspace_config: \"[Settings](Config/ws-settings.md)\"\n---\n".as_bytes(),
        )
        .unwrap();
    fs.write(
        Path::new("Config/ws-settings.md"),
        "---\ntitle: Settings\n---\n".as_bytes(),
    )
    .unwrap();
    let ws = Workspace::new(SyncToAsyncFs::new(fs));

    block_on_test(ws.set_workspace_config_field(Path::new("README.md"), "theme_mode", "dark"))
        .unwrap();

    // The field landed in the linked file, and no default Meta/Config.md was created.
    let settings = block_on_test(ws.fs.read_to_string(Path::new("Config/ws-settings.md"))).unwrap();
    assert!(
        settings.contains("theme_mode: dark"),
        "settings: {settings}"
    );
    assert!(
        block_on_test(ws.fs.read_to_string(Path::new("Meta/Config.md"))).is_err(),
        "default settings file should not have been created"
    );

    let config = block_on_test(ws.get_workspace_config(Path::new("README.md"))).unwrap();
    assert_eq!(config.theme_mode.as_deref(), Some("dark"));
}

#[test]
fn test_plugins_config_round_trips_through_settings_file() {
    // The per-plugin `plugins` config is a deeply nested mapping with
    // internal scalar sequences (permission include/exclude lists). It must
    // survive the comment-preserving write path into the settings file and
    // read back intact via get_workspace_config — and crucially must not
    // drop a top-level key written after it (the fig block-sequence hazard).
    let fs = InMemoryFileSystem::new();
    fs.write(
        Path::new("README.md"),
        "---\ntitle: Root\ncontents: []\n---\n".as_bytes(),
    )
    .unwrap();
    let ws = Workspace::new(SyncToAsyncFs::new(fs));

    let plugins_json = r#"{"diaryx.daily":{"permissions":{"read_files":{"include":["Daily","README.md"],"exclude":["Daily/secret.md"]},"edit_files":{"include":["Daily"],"exclude":[]}}},"diaryx.sync":{"permissions":{"http_requests":{"include":["*"],"exclude":[]}}}}"#;
    block_on_test(ws.set_workspace_config_field(Path::new("README.md"), "plugins", plugins_json))
        .unwrap();
    // A top-level scalar written AFTER the nested plugins mapping — must not
    // be swallowed when plugins' internal sequences are serialized.
    block_on_test(ws.set_workspace_config_field(Path::new("README.md"), "theme_mode", "dark"))
        .unwrap();

    // plugins lives in the settings file, not the root index README.
    let root = block_on_test(ws.fs.read_to_string(Path::new("README.md"))).unwrap();
    assert!(
        !root.contains("diaryx.daily"),
        "plugins leaked to root: {root}"
    );

    let config = block_on_test(ws.get_workspace_config(Path::new("README.md"))).unwrap();
    assert_eq!(
        config.theme_mode.as_deref(),
        Some("dark"),
        "top-level key after plugins was dropped"
    );

    let plugins = config.plugins.expect("plugins parsed after write");
    let daily = plugins
        .get("diaryx.daily")
        .and_then(|p| p.get("permissions"))
        .expect("diaryx.daily.permissions present");
    let read_include = daily
        .get("read_files")
        .and_then(|r| r.get("include"))
        .and_then(|i| i.as_sequence())
        .expect("read_files.include present");
    let read_include: Vec<&str> = read_include.iter().filter_map(|v| v.as_str()).collect();
    assert_eq!(read_include, vec!["Daily", "README.md"]);

    let read_exclude = daily
        .get("read_files")
        .and_then(|r| r.get("exclude"))
        .and_then(|i| i.as_sequence())
        .expect("read_files.exclude present");
    let read_exclude: Vec<&str> = read_exclude.iter().filter_map(|v| v.as_str()).collect();
    assert_eq!(read_exclude, vec!["Daily/secret.md"]);

    // The nested key that FOLLOWS the include/exclude sequences must survive.
    assert!(
        daily.get("edit_files").is_some(),
        "nested key after a sequence was dropped: {plugins:?}"
    );
    assert!(
        plugins.get("diaryx.sync").is_some(),
        "second plugin entry was dropped: {plugins:?}"
    );
}

#[test]
fn test_migrate_sweeps_lingering_plugins_from_already_linked_root() {
    // A workspace migrated before `plugins` joined WORKSPACE_CONFIG_FIELDS:
    // it already links to a settings file, but `plugins` still lingers in the
    // root index. Re-running migration must sweep it into the linked file and
    // strip it from the root.
    let fs = InMemoryFileSystem::new();
    fs.write(
            Path::new("README.md"),
            "---\ntitle: Root\ncontents: []\nworkspace_config: \"[Config](Meta/Config.md)\"\nplugins:\n  diaryx.daily:\n    permissions:\n      read_files:\n        include:\n          - Daily\n        exclude: []\n---\n\n# Root\n".as_bytes(),
        )
        .unwrap();
    fs.write(
        Path::new("Meta/Config.md"),
        "---\ntitle: Workspace Settings\ntheme_mode: dark\n---\n".as_bytes(),
    )
    .unwrap();
    let ws = Workspace::new(SyncToAsyncFs::new(fs));

    let migrated =
        block_on_test(ws.migrate_workspace_config_to_file(Path::new("README.md"))).unwrap();
    assert!(
        migrated,
        "lingering plugins should trigger a sweep migration"
    );

    let root = block_on_test(ws.fs.read_to_string(Path::new("README.md"))).unwrap();
    assert!(
        !root.contains("diaryx.daily"),
        "plugins not stripped from root: {root}"
    );
    assert!(root.contains("workspace_config:"), "link removed: {root}");

    let config = block_on_test(ws.get_workspace_config(Path::new("README.md"))).unwrap();
    assert!(config.plugins.is_some(), "plugins lost during sweep");
    assert_eq!(
        config.theme_mode.as_deref(),
        Some("dark"),
        "existing config clobbered"
    );

    // Sweep is idempotent once there is nothing left inline.
    let again = block_on_test(ws.migrate_workspace_config_to_file(Path::new("README.md"))).unwrap();
    assert!(!again, "second sweep should be a no-op");
}
