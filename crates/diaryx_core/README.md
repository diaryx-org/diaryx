---
title: diaryx_core
description: Core library shared by Diaryx clients
author: adammharris
part_of: '[README](/crates/README.md)'
contents:
- '[README](/crates/diaryx_core/src/README.md)'
exclude:
- '*.lock'
- '**/*.rs'
---
# Diaryx Core Library

This is the `diaryx_core` library! It contains shared code for the Diaryx clients.

## Async-first Architecture

This library uses an **async-first** design. All core modules (`Workspace`, `Validator`, `Exporter`, `Searcher`, `Publisher`) use the `AsyncFileSystem` trait for filesystem operations.

`diaryx_core` is platform-agnostic — the only `FileSystem` implementation it ships is the portable `InMemoryFileSystem`. Platform-specific implementations live in sibling crates:

- Native (`std::fs`): [`diaryx_native::RealFileSystem`](../diaryx_native/README.md)
- Browser (OPFS / IndexedDB / File System Access): `diaryx_wasm`

**For CLI/native code:** use [`diaryx_native`](../diaryx_native/README.md). It provides `RealFileSystem`, a `block_on` re-export, and a `NativeConfigExt` trait that restores `Config::load()` / `save()` / `init()` on native.

```rust,ignore
use diaryx_core::fs::SyncToAsyncFs;
use diaryx_core::workspace::Workspace;
use diaryx_native::{RealFileSystem, block_on};

let fs = SyncToAsyncFs::new(RealFileSystem);
let workspace = Workspace::new(fs);

// Use block_on for sync contexts
let tree = block_on(workspace.build_tree(Path::new("README.md")));
```

**For WASM:** Implement `AsyncFileSystem` directly using JS promises/IndexedDB. See `diaryx_wasm` for the browser-side storage backends.

## Quick overview

```markdown
diaryx_core
└── src
    ├── backup.rs ("Backup" is making a ZIP file of all the markdown files)
    ├── command.rs (Command pattern API for unified WASM/Tauri operations)
    ├── command_handler.rs (Command execution implementation)
    ├── config.rs (configuration for the core to share)
    ├── diaryx.rs (Central data structure used)
    ├── entry (Functionality to manipulate entries)
    │   ├── helpers.rs
    │   └── mod.rs
    ├── error.rs (Shared error types)
    ├── export.rs (Like backup, but filtering by "audience" trait)
    ├── frontmatter.rs (Operations to read and manipulate frontmatter in markdown files)
    ├── fs (Filesystem abstraction)
    │   ├── async_fs.rs (Async filesystem trait and SyncToAsyncFs adapter)
    │   ├── memory.rs (In-memory filesystem, used by WASM/web client)
    │   ├── mod.rs
    │   └── native.rs (Actual filesystem [std::fs] used by Tauri/CLI)
    ├── lib.rs
    ├── search.rs (Searching by frontmatter or content)
    ├── template.rs (Templating functionality for entry scaffolding)
    ├── test_utils.rs (Feature-gated unit test utility functions)
    ├── utils
    │   ├── date.rs (chrono for date and time manipulation)
    │   ├── mod.rs
    │   └── path.rs (finding relative paths, etc.)
    ├── validate.rs (Validating and fixing incorrectly organized workspaces)
    └── workspace (organizing collections of markdown files as "workspaces")
        ├── mod.rs
        └── types.rs
```

## Provided functionality

### Managing frontmatter

Full key-value operations for managing frontmatter properties:

- `set_frontmatter_property`
- `get_frontmatter_property`
- `rename_frontmatter_property`
- `remove_frontmatter_property`
- `get_all_frontmatter`

Also, sorting frontmatter properties:

- `sort_frontmatter`
- `sort_alphabetically`
- `sort_by_pattern`

## Managing file content

Operations for managing content of markdown files separate from frontmatter:

- `set_content`
- `get_content`
- `append_content`
- `clear_content`

## Search

Search frontmatter or content separately:

- `SearchQuery::content`
- `SearchQuery::frontmatter`

## Export

```rust,ignore
use diaryx_core::export::{ExportOptions, ExportPlan, Exporter};
use diaryx_core::fs::SyncToAsyncFs;
use diaryx_native::RealFileSystem;
use std::path::Path;

let workspace_root = Path::new("./workspace");
let audience = "public";
let destination = Path::new("./export");
let fs = SyncToAsyncFs::new(RealFileSystem);
let exporter = Exporter::new(fs);

// Use futures_lite::future::block_on for sync contexts
let plan = futures_lite::future::block_on(
    exporter.plan_export(&workspace_root, audience, destination, None)
).unwrap();

let force = false;
let keep_audience = false;
let options = ExportOptions {
    force,
    keep_audience,
};

let result = futures_lite::future::block_on(
    exporter.execute_export(&plan, &options)
);

match result {
  Ok(stats) => {
    println!("✓ {}", stats);
    println!("  Exported to: {}", destination.display());
  }
  Err(e) => {
    eprintln!("✗ Export failed: {}", e);
  }
}
```

## Validation

The `validate` module provides functionality to check workspace link integrity and automatically fix issues.

### Validator

The `Validator` struct checks `part_of` and `contents` references within a workspace:

```rust,ignore
use diaryx_core::validate::Validator;
use diaryx_core::fs::SyncToAsyncFs;
use diaryx_native::RealFileSystem;
use std::path::Path;

let fs = SyncToAsyncFs::new(RealFileSystem);
let validator = Validator::new(fs);

// Validate entire workspace starting from root index
// The second parameter controls orphan detection depth:
// - Some(2) matches tree view depth (recommended for UI)
// - None for unlimited depth (full workspace scan)
// Exclude patterns from the nearest index + its part_of ancestors are
// honored during that scan, including workspace-relative directory globs
// such as "**/target" and "**/dist/**". Validation also prunes common
// build/dependency directories such as target, node_modules, dist, build,
// and .git before recursing into them, plus hidden dot-directories such as
// .direnv and .zig-cache.
// "Show All Files" / filesystem-tree mode now uses the same exclude inheritance
// and built-in directory pruning rules.
let root_path = Path::new("./workspace/README.md");
let result = futures_lite::future::block_on(
    validator.validate_workspace(&root_path, Some(2))
).unwrap();

// Or validate a single file
let file_path = Path::new("./workspace/notes/my-note.md");
let result = futures_lite::future::block_on(
    validator.validate_file(&file_path)
).unwrap();

if result.is_ok() {
    println!("✓ Validation passed ({} files checked)", result.files_checked);
} else {
    println!("Found {} errors and {} warnings",
             result.errors.len(),
             result.warnings.len());
}
```

#### Validation Errors

- `BrokenPartOf` - A file's `part_of` points to a non-existent file
- `BrokenContentsRef` - An index's `contents` references a non-existent file
- `BrokenAttachment` - A file's `attachments` references a non-existent file

#### Validation Warnings

- `OrphanFile` - A markdown file not referenced by any index's `contents`. When validating a single index file, this also flags sub-indexes (files with a `contents` property) sitting in immediate subdirectories that the current index doesn't reference.
- `UnlinkedEntry` - A file/directory not in the contents hierarchy
- `CircularReference` - Circular reference detected in workspace hierarchy
- `NonPortablePath` - A path contains absolute paths or `.`/`..` components
- `MultipleIndexes` - Multiple index files in the same directory
- `OrphanBinaryFile` - A binary file not referenced by any index's `attachments`
- `MissingPartOf` - A non-index file has no `part_of` property

#### Exclude Patterns

Index files can define `exclude` patterns to suppress `OrphanFile` and `OrphanBinaryFile` warnings for specific files:

```yaml
---
title: Docs
contents:
  - guide.md
exclude:
  - "LICENSE.md"        # Exact filename
  - "*.lock"            # Glob pattern
  - "build/**"          # Recursive glob
---
```

Exclude patterns are **inherited** up the `part_of` hierarchy. If a parent index excludes `*.lock` files, that pattern also applies to all child directories.

### ValidationFixer

The `ValidationFixer` struct provides methods to automatically fix validation issues:

```rust,ignore
use diaryx_core::validate::{Validator, ValidationFixer};
use diaryx_core::fs::SyncToAsyncFs;
use diaryx_native::RealFileSystem;
use std::path::Path;

let fs = SyncToAsyncFs::new(RealFileSystem);
let validator = Validator::new(fs.clone());
let fixer = ValidationFixer::new(fs);

// Validate workspace (use None for full depth when fixing)
let root_path = Path::new("./workspace/README.md");
let result = futures_lite::future::block_on(
    validator.validate_workspace(&root_path, None)
).unwrap();

// Fix all issues at once
let (error_fixes, warning_fixes) = futures_lite::future::block_on(
    fixer.fix_all(&result)
);

for fix in error_fixes.iter().chain(warning_fixes.iter()) {
    if fix.success {
        println!("✓ {}", fix.message);
    } else {
        println!("✗ {}", fix.message);
    }
}

// Or fix individual issues (all methods are async)
futures_lite::future::block_on(async {
    fixer.fix_broken_part_of(Path::new("./file.md")).await;
    fixer.fix_broken_contents_ref(Path::new("./index.md"), "missing.md").await;
    fixer.fix_unlisted_file(Path::new("./index.md"), Path::new("./new-file.md")).await;
    fixer.fix_missing_part_of(Path::new("./orphan.md"), Path::new("./index.md")).await;
});
```

## Templates

Templates provide reusable content patterns for new entries.

### Template Syntax

Templates support variable substitution:

- `{{title}}` - Entry title
- `{{filename}}` - Filename without extension
- `{{date}}` - Current date (ISO format)
- `{{part_of}}` - Parent index reference

### Built-in Templates

- `note` - General note with title placeholder

### Using Templates

```rust,ignore
use diaryx_core::template::{TemplateManager, TemplateContext};
use diaryx_core::fs::InMemoryFileSystem;

let fs = InMemoryFileSystem::new();
let manager = TemplateManager::new(&fs)
    .with_workspace_dir(Path::new("/workspace"));

// Get a template
let template = manager.get("note").unwrap();

// Render with context
let context = TemplateContext::new()
    .with_title("January 15, 2024")
    .with_date(date)
    .with_part_of("2024_january.md");

let content = template.render(&context);
```

### Workspace Config Templates

Templates can be regular workspace entries referenced by link in workspace config:

```yaml
default_template: "[Default](/templates/default.md)"
```

The resolution order is: workspace config link -> `_templates/` directory -> built-in templates.

### Custom Templates (Legacy)

Create custom templates in `_templates/` within your workspace:

```markdown
---
title: {{title}}
part_of: {{part_of}}
tags: []
---

# {{title}}

Created: {{date}}
```

## Workspaces

Workspaces organize entries into a tree structure using `part_of` and `contents` relationships.

### Tree Structure

```rust,ignore
use diaryx_core::workspace::Workspace;
use diaryx_core::fs::SyncToAsyncFs;
use diaryx_native::RealFileSystem;
use std::path::Path;

let fs = SyncToAsyncFs::new(RealFileSystem);
let workspace = Workspace::new(fs);

// Build tree from root index
let tree = futures_lite::future::block_on(
    workspace.build_tree(Path::new("README.md"))
)?;

// Traverse the tree
for child in &tree.children {
    println!("{}: {}", child.path.display(), child.title);
}
```

### Link Formats

Configure how `part_of`, `contents`, and `attachments` links are formatted:

- `LinkFormat::MarkdownRoot` (default) - `[../parent.md](../parent.md)` (clickable in editors)
- `LinkFormat::Relative` - `../parent.md` (simple relative paths)
- `LinkFormat::Absolute` - `/workspace/parent.md` (absolute from workspace root)

## Date utilities

The `date` module provides timestamp formatting helpers:

```rust,ignore
use diaryx_core::date::{current_local_timestamp_rfc3339, timestamp_millis_to_local_rfc3339};

let now = current_local_timestamp_rfc3339();
let from_millis = timestamp_millis_to_local_rfc3339(1_700_000_000_000);
```

Natural language date parsing ("today", "3 days ago", etc.) is provided by the `diaryx_daily_extism` plugin.

## Shared errors

The `error` module provides [`DiaryxError`] for all fallible operations:

```rust,ignore
use diaryx_core::error::{DiaryxError, Result};

fn example() -> Result<()> {
    // Operations return Result<T, DiaryxError>
    let content = fs.read_to_string(path)?;
    Ok(())
}

// Error handling
match result {
    Err(DiaryxError::FileRead { path, source }) => {
        eprintln!("Failed to read {}: {}", path.display(), source);
    }
    Err(DiaryxError::NoFrontmatter(path)) => {
        // Handle missing frontmatter gracefully
    }
    Err(DiaryxError::InvalidDateFormat(input)) => {
        eprintln!("Invalid date: {}", input);
    }
    _ => {}
}
```

For IPC (Tauri), convert to `SerializableError`:

```rust,ignore
let serializable = error.to_serializable();
// { kind: "FileRead", message: "...", path: Some(...) }
```

## Configuration

Diaryx uses a two-layer configuration model plus a separate native auth store:

- **User config** (`~/.config/diaryx/config.md`) - Device/user-level settings such as default workspace and editor. Stored as markdown with YAML frontmatter. Managed by the `config` module.
- **Native auth store** (`~/.config/diaryx/auth.md`) - Device/user-level Diaryx account session and remembered sync server/workspace metadata. Managed by the `auth` module on native hosts.
- **Workspace config** (root index frontmatter) - Workspace-level settings (link format, filename style, templates, audience). Managed by `WorkspaceConfig` in the `workspace` module.

### User Config

```rust,ignore
use diaryx_core::config::Config;
use diaryx_native::NativeConfigExt; // brings Config::load() into scope
use std::path::PathBuf;

let config = Config::load()?;

let workspace = &config.default_workspace;    // Main workspace path
let editor = &config.editor;                  // Preferred editor
```

```toml
default_workspace = "/home/user/diary"
editor = "nvim"

# Sync settings (optional)
sync_server_url = "https://sync.example.com"
sync_email = "user@example.com"
```

### Workspace Config

Workspace-level settings live in the root index file's YAML frontmatter. See [workspace/README.md](src/workspace/README.md) for the full field reference.

```yaml
---
title: My Workspace
link_format: markdown_root
filename_style: kebab_case
auto_update_timestamp: true
auto_rename_to_title: true
sync_title_to_heading: false
default_template: "[Default](/templates/default.md)"
public_audience: "public"
---
```

## Filesystem abstraction

The `fs` module provides filesystem abstraction through two traits: `FileSystem` (synchronous) and `AsyncFileSystem` (asynchronous).

**Note:** As of the async-first refactor, all core modules (`Workspace`, `Validator`, `Exporter`, `Searcher`, `Publisher`) use `AsyncFileSystem`. For synchronous contexts (CLI, tests), wrap a sync filesystem with `SyncToAsyncFs` and use `futures_lite::future::block_on()`.

### FileSystem trait

The synchronous `FileSystem` trait provides basic implementations:

- [`diaryx_native::RealFileSystem`](../diaryx_native/README.md) - Native filesystem using `std::fs` (lives in the `diaryx_native` crate; not available on WASM)
- `InMemoryFileSystem` - In-memory implementation, useful for WASM and testing

```rust,ignore
use diaryx_core::fs::{FileSystem, InMemoryFileSystem};
use std::path::Path;

// Create an in-memory filesystem
let fs = InMemoryFileSystem::new();

// Write a file (sync)
fs.write_file(Path::new("workspace/README.md"), "# Hello").unwrap();

// Read it back
let content = fs.read_to_string(Path::new("workspace/README.md")).unwrap();
assert_eq!(content, "# Hello");
```

### AsyncFileSystem trait (Primary API)

The `AsyncFileSystem` trait is the primary API for all core modules:

- WASM environments where JavaScript APIs (like IndexedDB) are async
- Native code using async runtimes like tokio
- All workspace operations (Workspace, Validator, Exporter, etc.)

```rust,ignore
use diaryx_core::fs::{AsyncFileSystem, InMemoryFileSystem, SyncToAsyncFs};
use diaryx_core::workspace::Workspace;
use std::path::Path;

// Wrap a sync filesystem for use with async APIs
let sync_fs = InMemoryFileSystem::new();
let async_fs = SyncToAsyncFs::new(sync_fs);

// Use with Workspace (async)
let workspace = Workspace::new(async_fs);

// For sync contexts, use block_on
let tree = futures_lite::future::block_on(
    workspace.build_tree(Path::new("README.md"))
);
```

### SyncToAsyncFs adapter

The `SyncToAsyncFs` struct wraps any synchronous `FileSystem` implementation to provide an `AsyncFileSystem` interface. This is the recommended way to use the async-first API in synchronous contexts:

```rust,ignore
use diaryx_core::fs::{InMemoryFileSystem, SyncToAsyncFs};
use diaryx_native::RealFileSystem;
use diaryx_core::workspace::Workspace;

// For native code
let fs = SyncToAsyncFs::new(RealFileSystem);
let workspace = Workspace::new(fs);

// For tests/WASM
let fs = SyncToAsyncFs::new(InMemoryFileSystem::new());
let workspace = Workspace::new(fs);

// Access the inner sync filesystem if needed
// let inner = async_fs.inner();
```

&nbsp;
