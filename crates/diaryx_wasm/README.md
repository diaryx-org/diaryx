---
title: diaryx_wasm
part_of: ../../README.md
audience:
  - developers
---

# diaryx_wasm

WebAssembly bindings for `diaryx_core`, used by the web frontend in `apps/web`.

## Building

To build the WebAssembly module:

```bash
wasm-pack build --target web --out-dir ../../apps/web/src/lib/wasm
```

## Architecture

The crate provides typed class-based APIs that wrap `diaryx_core` functionality:

| Class                    | Purpose                                   |
| ------------------------ | ----------------------------------------- |
| `DiaryxWorkspace`        | Workspace tree operations                 |
| `DiaryxEntry`            | Entry CRUD operations                     |
| `DiaryxFrontmatter`      | Frontmatter manipulation                  |
| `DiaryxSearch`           | Workspace search                          |
| `DiaryxTemplate`         | Template management                       |
| `DiaryxValidation`       | Link integrity validation and fixing      |
| `DiaryxExport`           | Export with audience filtering            |
| `DiaryxAttachment`       | Attachment upload/download                |
| `DiaryxFilesystem`       | Low-level filesystem operations (sync)    |
| `DiaryxAsyncFilesystem`  | Async filesystem operations with Promises |

### In-Memory Filesystem

Unlike the CLI and Tauri backends which use `RealFileSystem` (native filesystem), the WASM backend uses `InMemoryFileSystem`. This allows the web app to:

1. Load files from IndexedDB on startup
2. Operate entirely in memory during use
3. Persist changes back to IndexedDB

## API Reference

### DiaryxValidation

Validates workspace link integrity and fixes issues.

```javascript
import init, { DiaryxValidation } from "./wasm/diaryx_wasm.js";

await init();
const validation = new DiaryxValidation();

// Validate entire workspace
const result = validation.validate("workspace");
console.log(`Checked ${result.files_checked} files`);
console.log(`Errors: ${result.errors.length}`);
console.log(`Warnings: ${result.warnings.length}`);

// Validate single file
const fileResult = validation.validate_file("workspace/notes/my-note.md");

// Fix all issues
const fixSummary = validation.fix_all(result);
console.log(
  `Fixed: ${fixSummary.total_fixed}, Failed: ${fixSummary.total_failed}`,
);

// Fix individual issues
validation.fix_broken_part_of("workspace/broken.md");
validation.fix_broken_contents_ref("workspace/index.md", "missing.md");
validation.fix_unlisted_file("workspace/index.md", "workspace/unlisted.md");
validation.fix_missing_part_of("workspace/orphan.md", "workspace/index.md");
```

#### Validation Errors

- `BrokenPartOf` - `part_of` points to non-existent file
- `BrokenContentsRef` - `contents` references non-existent file
- `BrokenAttachment` - `attachments` references non-existent file

#### Validation Warnings

- `OrphanFile` - Markdown file not in any index's contents
- `UnlinkedEntry` - File/directory not in contents hierarchy
- `UnlistedFile` - File in directory but not in index's contents
- `CircularReference` - Circular reference in hierarchy
- `NonPortablePath` - Path contains absolute or `.`/`..` components
- `MultipleIndexes` - Multiple index files in same directory
- `OrphanBinaryFile` - Binary file not in any attachments
- `MissingPartOf` - Non-index file has no `part_of`

### Legacy API

For backwards compatibility, standalone functions are also exported:

```javascript
import {
  validate_workspace,
  validate_file,
  fix_all_validation_issues,
} from "./wasm/diaryx_wasm.js";

const result = validate_workspace("workspace");
const fixSummary = fix_all_validation_issues(result);
```

### DiaryxAsyncFilesystem

Async filesystem operations that return JavaScript Promises. This is useful for consistent async/await patterns in JavaScript and future integration with truly async storage (e.g., IndexedDB).

```javascript
import init, { DiaryxAsyncFilesystem } from "./wasm/diaryx_wasm.js";

await init();
const asyncFs = new DiaryxAsyncFilesystem();

// All methods return Promises
const content = await asyncFs.read_file("workspace/README.md");
await asyncFs.write_file("workspace/new.md", "# New File");
const exists = await asyncFs.file_exists("workspace/new.md");

// Directory operations
await asyncFs.create_dir_all("workspace/notes/2024");
const isDir = await asyncFs.is_dir("workspace/notes");

// List files
const mdFiles = await asyncFs.list_md_files("workspace");
console.log(`Found ${mdFiles.count} markdown files:`, mdFiles.files);

// Recursive listing
const allMd = await asyncFs.list_md_files_recursive("workspace");
const allFiles = await asyncFs.list_all_files_recursive("workspace");

// Binary file operations
const data = await asyncFs.read_binary("workspace/image.png");
await asyncFs.write_binary("workspace/copy.png", data);

// Bulk operations for IndexedDB sync
const backupData = await asyncFs.get_backup_data();
// ... persist to IndexedDB ...
await asyncFs.restore_from_backup(backupData);

// Load/export files
await asyncFs.load_files([
  ["workspace/README.md", "# Hello"],
  ["workspace/notes.md", "# Notes"],
]);
const entries = await asyncFs.export_files();

// Clear filesystem
await asyncFs.clear();
```

#### Async vs Sync Filesystem

- `DiaryxFilesystem` - Synchronous methods, returns values directly
- `DiaryxAsyncFilesystem` - All methods return Promises

While the underlying `InMemoryFileSystem` is synchronous, `DiaryxAsyncFilesystem` provides a Promise-based API that:

1. Enables consistent async/await patterns in JavaScript
2. Allows for future integration with truly async operations
3. Works well with JavaScript's event loop

## Error Handling

All methods return `Result<T, JsValue>` for JavaScript interop. Errors are converted to JavaScript exceptions with descriptive messages.
