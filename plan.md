# Plan: Web Import from diaryx_core

## Context

`diaryx_core` has a new `import` module with pure parsers for three external formats:
- **Email** (`.eml`, `.mbox`) — feature `import-email`
- **Day One** (`Journal.json`) — feature `import-dayone`
- **Markdown** (directory of `.md` files) — feature `import-markdown`

These parsers return `ImportedEntry` values (title, date, body, metadata, attachments). Currently, only the CLI (`crates/diaryx/src/cli/import.rs`) consumes them — the web client has no access. The web already has a ZIP-based import in `ImportSettings.svelte`, but that's a raw file extraction, not a structured format-aware import.

## Architecture Overview

The import flow has two distinct concerns:
1. **Parsing**: Pure functions that take bytes and return `ImportedEntry` — already in `diaryx_core::import`
2. **Orchestration**: Reading source files, writing entries with frontmatter/`part_of`/`contents`, building the workspace index hierarchy — currently only in the CLI handler (`crates/diaryx/src/cli/import.rs`, ~600 lines of filesystem logic)

The CLI orchestration is tightly coupled to `std::fs` and `std::path`. The web uses an async filesystem abstraction (`AsyncFileSystem` trait). The orchestration logic needs to be made reusable.

## Recommended Approach

### Step 1: Add `ImportEntries` command to `diaryx_core::Command`

Add a new command variant to the existing `Command` enum (`crates/diaryx_core/src/command.rs`):

```rust
/// Import pre-parsed entries into the workspace, building the date-based
/// hierarchy (indexes, part_of links, attachments).
ImportEntries {
    /// Serialized Vec<ImportedEntry> (JSON).
    entries_json: String,
    /// Base folder name for the imported entries (e.g. "emails", "journal").
    folder: String,
}
```

This keeps parsing on the JS/WASM side (where the user selects files) and delegates the filesystem orchestration to Rust, which already knows how to build the workspace hierarchy.

### Step 2: Extract shared orchestration into `diaryx_core`

Move the `write_entries` and `write_index_hierarchy` logic from `crates/diaryx/src/cli/import.rs` into `diaryx_core::import` (e.g., a new `orchestrate.rs` module), refactored to use the `AsyncFileSystem` trait instead of `std::fs`. This is the largest piece of work:

- `write_entries()` → async version using `self.fs().write_file()`, `self.fs().create_dir_all()`, etc.
- `write_index_hierarchy()` → same async refactor
- Helper functions (`date_components`, `entry_slug`, `deduplicate_path`, `format_entry`) stay as-is

**Critical: Graft into existing workspace hierarchy.** The current CLI handler creates a self-contained sub-tree (e.g., `emails/index.md` with internal `contents`/`part_of` links), but does **not** connect it to the workspace root. This means imported entries are invisible in the web sidebar — the tree walks from the root index via `contents`, and the new folder isn't listed. Validation would flag `MissingPartOf` and `OrphanFile`/`UnlinkedEntry` warnings (both auto-fixable), but this is a poor default experience.

The orchestration must add a final "grafting" step:
1. Locate the workspace root index (via `find_root_index_in_dir`)
2. Read its frontmatter and append the import folder's index to `contents` (if not already present)
3. Set `part_of` on the import folder's root index pointing back to the workspace root

This ensures imported entries appear in the sidebar immediately, without requiring the user to run validation + auto-fix.

The existing CLI handler should then be simplified to call the shared orchestration, converting from `std::fs` to the `StdFileSystem` adapter that `diaryx_core` already provides.

Implement the `ImportEntries` command handler in `command_handler.rs` to call this shared orchestration.

### Step 3: Enable import features in `diaryx_wasm`

In `crates/diaryx_wasm/Cargo.toml`, add the import features to the `diaryx_core` dependency:

```toml
diaryx_core = { path = "../diaryx_core", features = ["crdt", "import-dayone", "import-markdown"] }
```

**Omit `import-email`** initially — `parse_mbox` uses memory-mapped I/O which is unavailable in WASM. Only `parse_eml` works, and `html-to-markdown-rs` falls back to raw HTML on WASM. Email support can be added later with a WASM-compatible mbox parser or by only exposing `.eml` parsing.

### Step 4: Add WASM-side parsing bridge functions

Add `#[wasm_bindgen]` functions in `crates/diaryx_wasm/src/backend.rs` (or a new `import.rs` module) that expose the parsers to JavaScript:

```rust
#[wasm_bindgen(js_name = "parseDayOneJson")]
pub fn parse_dayone_json(bytes: &[u8]) -> Result<String, JsValue> {
    let results = diaryx_core::import::dayone::parse_dayone(bytes);
    // Serialize results as JSON for JS consumption
    serde_json::to_string(&results).map_err(...)
}

#[wasm_bindgen(js_name = "parseMarkdownFile")]
pub fn parse_markdown_file(bytes: &[u8], filename: &str) -> Result<String, JsValue> {
    let entry = diaryx_core::import::markdown::parse_markdown_file(bytes, filename)?;
    serde_json::to_string(&entry).map_err(...)
}
```

Alternatively, parsing can be done entirely in the `ImportEntries` command handler if source bytes are passed in — but this would mean serializing potentially large binary data through the command JSON. The two-step approach (parse client-side → send structured entries → orchestrate server-side) keeps message sizes smaller.

### Step 5: Build the web UI

Add a new `FormatImportSettings.svelte` component (or extend `ImportSettings.svelte`) in `apps/web/src/lib/settings/`:

**UI Design:**
- Add a section in the existing "Data" settings tab, below the current ZIP import
- Or: add a new dedicated "Import" tab to SettingsDialog
- Format selector (Day One / Markdown folder) — a dropdown or button group
- File input:
  - Day One: single `Journal.json` file picker
  - Markdown: either individual `.md` files or a `.zip` of a directory
- Target folder name input (text field, with sensible default per format)
- Progress bar + result summary (reuse the existing `ImportSettings` patterns)
- Confirmation dialog before import begins

**File picker considerations:**
- Web browsers cannot pick directories via `<input type="file">`. For markdown directory import, the options are:
  1. Accept a `.zip` of the directory (simplest, recommended)
  2. Use `<input type="file" webkitdirectory>` (Chrome/Edge/Firefox support it, but not Safari on iOS)
  3. Use the File System Access API `showDirectoryPicker()` (Chrome/Edge only)
- **Recommendation:** Accept `.zip` for markdown directory import. This aligns with the existing ZIP import pattern and works everywhere.

**Flow:**
1. User selects format and file(s)
2. JS reads file bytes via `FileReader`
3. Call WASM parser (e.g., `parseDayOneJson(bytes)`) to get `ImportedEntry[]`
4. Display preview: count of entries, date range, any parse errors
5. User confirms import, optionally sets folder name
6. Call `backend.execute({ type: 'ImportEntries', params: { entries_json, folder } })`
7. Show progress and result

### Step 6: Handle attachments

Day One exports include attachment references (photos/videos), but the actual binary files are in a separate `photos/` directory in the export ZIP. The web import should:

1. For Day One: accept the full export ZIP, extract `Journal.json` for parsing, then extract media files from `photos/` and write them as attachments
2. For Markdown: extract non-`.md` files from the ZIP and write them alongside entries

This means the orchestration layer needs to handle attachment bytes. `ImportedEntry.attachments` already carries `Vec<ImportedAttachment>` with raw bytes, so the plumbing is there.

### Step 7: Add `ImportedEntry` serde support

`ImportedEntry`, `ImportedAttachment`, `ImportResult`, and `ImportOptions` in `diaryx_core::import::mod.rs` currently lack `Serialize`/`Deserialize` derives. Add them:

```rust
#[derive(Serialize, Deserialize)]
pub struct ImportedEntry { ... }
```

This is needed for the JSON serialization bridge between JS and the WASM command handler.

## WASM-Specific Concerns

| Issue | Impact | Mitigation |
|-------|--------|------------|
| `parse_mbox` uses `mmap` | Won't compile for WASM | Gate behind `#[cfg(not(target_arch = "wasm32"))]` or skip email import entirely for now |
| `html-to-markdown-rs` unavailable | Email HTML bodies returned as raw HTML | Already handled with `#[cfg]` fallback |
| Large file handling | Day One exports can be hundreds of MB | Use streaming ZIP extraction (already in `ImportSettings`); parse entries in batches if needed |
| Attachment binary data in command JSON | Base64 overhead doubles memory | Consider a two-phase approach: parse entries first, then stream attachments separately via existing `uploadAttachment` API |

## File Changes Summary

| File | Change |
|------|--------|
| `crates/diaryx_core/src/import/mod.rs` | Add `Serialize`/`Deserialize` derives to structs |
| `crates/diaryx_core/src/import/orchestrate.rs` | **New** — shared async orchestration logic extracted from CLI |
| `crates/diaryx_core/src/command.rs` | Add `ImportEntries` variant |
| `crates/diaryx_core/src/command_handler.rs` | Handle `ImportEntries` command |
| `crates/diaryx_wasm/Cargo.toml` | Enable `import-dayone`, `import-markdown` features |
| `crates/diaryx_wasm/src/backend.rs` | Add WASM parsing bridge functions |
| `crates/diaryx/src/cli/import.rs` | Refactor to use shared orchestration |
| `apps/web/src/lib/settings/FormatImportSettings.svelte` | **New** — UI for format-aware import |
| `apps/web/src/lib/settings/SettingsDialog.svelte` | Wire in new component to "data" tab |

## Phased Rollout Recommendation

**Phase 1 (MVP):** Day One + Markdown import via ZIP
- Serde derives, shared orchestration, `ImportEntries` command, WASM features, basic UI
- Most value for least effort — these two formats have no WASM blockers

**Phase 2:** Email `.eml` import
- Gate `parse_mbox` behind `#[cfg(not(target_arch = "wasm32"))]`
- Expose only `parse_eml` in WASM
- Accept individual `.eml` files or a ZIP of `.eml` files
- Handle HTML fallback gracefully in the UI

**Phase 3:** Enhanced UX
- Import preview (show parsed entries before committing)
- Drag-and-drop onto the main editor area
- Command palette integration (`Ctrl+Shift+I` → import)
- Progress streaming for large imports
