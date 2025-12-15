# Contributing to Diaryx

Welcome to the Diaryx project! This document will help you understand the codebase structure, identify areas for improvement, and find good first issues to work on.

## Repository Structure

Diaryx is organized as a Rust workspace with multiple crates:

```
diaryx-core/
├── crates/
│   ├── diaryx_core/     # Core library - shared logic for all frontends
│   ├── diaryx/          # CLI application
│   └── diaryx_wasm/     # WebAssembly bindings for web frontend
├── apps/
│   ├── tauri/           # Desktop application (Tauri)
│   └── web/             # Web application
└── Cargo.toml           # Workspace configuration
```

### Crate Overview

#### `diaryx_core` - Core Library

The heart of the project. Contains all business logic that should be shared across frontends.

| Module         | Purpose                                                                             |
| -------------- | ----------------------------------------------------------------------------------- |
| `config.rs`    | Configuration management (workspace paths, editor settings)                         |
| `date.rs`      | Natural language date parsing and path generation                                   |
| `entry.rs`     | Main `DiaryxApp` struct with entry CRUD operations                                  |
| `error.rs`     | Unified error types (`DiaryxError`)                                                 |
| `export.rs`    | Audience-based export functionality                                                 |
| `fs.rs`        | Filesystem abstraction (`FileSystem` trait, `RealFileSystem`, `InMemoryFileSystem`) |
| `publish.rs`   | HTML publishing with navigation                                                     |
| `search.rs`    | Full-text and frontmatter search                                                    |
| `template.rs`  | Template engine with variable substitution                                          |
| `workspace.rs` | Workspace tree building and index management                                        |

#### `diaryx` - CLI Application

Command-line interface built on top of `diaryx_core`.

| Module             | Purpose                                       |
| ------------------ | --------------------------------------------- |
| `main.rs`          | Entry point                                   |
| `editor.rs`        | System editor integration                     |
| `cli/args.rs`      | Clap argument definitions                     |
| `cli/mod.rs`       | Command dispatcher                            |
| `cli/entry.rs`     | today, yesterday, open, create commands       |
| `cli/workspace.rs` | workspace subcommands (add, mv, create, etc.) |
| `cli/property.rs`  | Frontmatter property manipulation             |
| `cli/content.rs`   | Body content manipulation                     |
| `cli/search.rs`    | Search command handler                        |
| `cli/template.rs`  | Template management                           |
| `cli/util.rs`      | Shared CLI utilities                          |

#### `diaryx_wasm` - WebAssembly Bindings

WASM bindings that expose `diaryx_core` functionality to JavaScript. Uses an in-memory filesystem that syncs with IndexedDB.

---

## Code Analysis & Issues

### 🔴 Critical: Code Duplication

The following functions are duplicated across crates and should be consolidated into `diaryx_core`:

#### 1. `prettify_filename`

**Location A:** `diaryx_core/src/entry.rs` (L683-696)
**Location B:** `diaryx/src/cli/workspace.rs` (L1726-1739)

Both implementations are identical:

```rust
fn prettify_filename(filename: &str) -> String {
    filename
        .replace(['-', '_'], " ")
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars).collect(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
```

**Fix:** Export from `diaryx_core` and use in CLI.

#### 2. Index Contents Management

**Core:** `DiaryxApp::add_to_index_contents` (entry.rs L636-664)
**WASM:** `add_to_index_contents` (lib.rs L469-511) and `add_to_parent_index` (lib.rs L357-416)

The WASM module reimplements this logic instead of using the core implementation.

**Additional Issue:** `remove_from_index_contents` exists only in WASM (L630-669), not in core!

#### 3. Relative Path Calculation

Three separate implementations:

- `diaryx_wasm/src/lib.rs` L515-541 (`relative_path_from_dir_to_target`)
- `diaryx_wasm/src/lib.rs` L673-705 (`relative_path_from_entry_to_target`)
- `diaryx/src/cli/util.rs` L229-270 (`calculate_relative_path`)

**Fix:** Create a unified `path_utils` module in `diaryx_core`.

#### 4. Frontmatter Parsing

**Core:** `DiaryxApp::parse_file` (entry.rs L97-126)
**WASM:** `parse_frontmatter` (lib.rs L957-994)

WASM has its own parsing instead of leveraging core's implementation.

### 🟡 Architectural Issues

#### WASM Layer Is Too Thick

The `diaryx_wasm` module (1057 lines) reimplements significant business logic:

- Entry creation with templates and parent linking
- Move operations with index updates
- Entry attachment/relationship management

**Ideal State:** WASM should be a thin wrapper that:

1. Manages the in-memory filesystem
2. Calls `diaryx_core` functions
3. Converts between Rust and JS types

#### Missing Higher-Level APIs in Core

The following operations are implemented in WASM/CLI but should be in `diaryx_core`:

| Operation                           | Current Location | Should Be In             |
| ----------------------------------- | ---------------- | ------------------------ |
| `attach_entry_to_parent`            | WASM only        | `diaryx_core::workspace` |
| `move_entry` (with index updates)   | WASM only        | `diaryx_core::workspace` |
| `remove_from_index_contents`        | WASM only        | `diaryx_core::entry`     |
| Path resolution with fuzzy matching | CLI only         | `diaryx_core` (optional) |

#### Test Infrastructure Duplication

`MockFileSystem` is defined separately in:

- `entry.rs` tests
- `search.rs` tests
- `export.rs` tests

**Fix:** Create `diaryx_core/src/test_utils.rs` with shared test infrastructure.

### 🟢 Minor Issues

1. **Missing Documentation:** Most public items lack rustdoc comments
2. **Inconsistent Error Handling:** Some WASM functions swallow errors
3. **No Input Validation:** Workspace operations don't validate paths thoroughly

---

## Good First Issues

Here are beginner-friendly tasks to get started with the codebase:

### 🟢 Easy (Documentation & Cleanup)

#### Issue 1: Export `prettify_filename` from Core

**Difficulty:** Easy  
**Files:** `diaryx_core/src/entry.rs`, `diaryx/src/cli/workspace.rs`  
**Task:**

1. Make `prettify_filename` in `entry.rs` public
2. Re-export it from `lib.rs`
3. Replace the duplicate in `workspace.rs` with an import

#### Issue 2: Add Rustdoc to `diaryx_core::error`

**Difficulty:** Easy  
**Files:** `diaryx_core/src/error.rs`  
**Task:** Add documentation comments to all error variants explaining when they occur.

#### Issue 3: Add Rustdoc to `diaryx_core::date`

**Difficulty:** Easy  
**Files:** `diaryx_core/src/date.rs`  
**Task:** Document the module, functions, and provide examples in doc comments.

#### Issue 4: Document Template Variables

**Difficulty:** Easy  
**Files:** `diaryx_core/src/template.rs`  
**Task:** Expand `TEMPLATE_VARIABLES` documentation and add usage examples.

### 🟡 Medium (Code Consolidation)

#### Issue 5: Create Shared Test MockFileSystem

**Difficulty:** Medium  
**Files:** New file `diaryx_core/src/test_utils.rs`, update test modules  
**Task:**

1. Create a `test_utils` module with a shared `MockFileSystem`
2. Feature-gate it with `#[cfg(test)]`
3. Update tests in `entry.rs`, `search.rs`, `export.rs` to use it

#### Issue 6: Add `remove_from_index_contents` to Core

**Difficulty:** Medium  
**Files:** `diaryx_core/src/entry.rs`, `diaryx_wasm/src/lib.rs`  
**Task:**

1. Port `remove_from_index_contents` from WASM to `DiaryxApp`
2. Update WASM to use the core implementation
3. Add tests

#### Issue 7: Create Path Utilities Module in Core

**Difficulty:** Medium  
**Files:** New file `diaryx_core/src/path_utils.rs`  
**Task:**

1. Create `relative_path_from_to(from: &Path, to: &Path) -> String`
2. Consolidate the three implementations
3. Update WASM and CLI to use the shared function

#### Issue 8: Use Core's Frontmatter Parsing in WASM

**Difficulty:** Medium  
**Files:** `diaryx_wasm/src/lib.rs`  
**Task:** Replace `parse_frontmatter` and `extract_body` with calls to `DiaryxApp` methods.

### 🔴 Advanced (Architectural Changes)

#### Issue 9: Move `attach_entry_to_parent` to Core

**Difficulty:** Hard  
**Files:** `diaryx_core/src/workspace.rs`, `diaryx_wasm/src/lib.rs`  
**Task:**

1. Add `attach_entry_to_parent` method to `Workspace` struct
2. Handle bidirectional linking (contents + part_of)
3. Update WASM to use the core implementation
4. Add comprehensive tests

#### Issue 10: Add Link Validation Command

**Difficulty:** Hard  
**Files:** New module in `diaryx_core`, new command in `diaryx`  
**Task:** (From roadmap)

1. Create `diaryx_core::validate` module
2. Check all `part_of`/`contents` references exist
3. Add `diaryx workspace validate` command

#### Issue 11: Move Entry with Reference Updates (Core)

**Difficulty:** Hard  
**Files:** `diaryx_core/src/workspace.rs`  
**Task:**

1. Add `move_entry` to `Workspace` that:
   - Moves the file
   - Updates old parent's contents
   - Updates new parent's contents
   - Updates the moved file's `part_of`
2. Update CLI and WASM to use it

---

## Development Setup

```bash
# Clone the repository
git clone https://github.com/diaryx-org/diaryx-core.git
cd diaryx-core

# Build all crates
cargo build

# Run tests
cargo test

# Install the CLI locally
cargo install --path crates/diaryx

# Build WASM (requires wasm-pack)
wasm-pack build crates/diaryx_wasm --target web
```

## Code Style

- Run `cargo fmt` before committing
- Run `cargo clippy` and address warnings
- Add tests for new functionality
- Document public APIs with rustdoc

## Pull Request Guidelines

1. **One issue per PR** - Keep changes focused
2. **Include tests** - Especially for bug fixes
3. **Update documentation** - If behavior changes
4. **Reference the issue** - Use "Fixes #123" in PR description

---

## Architecture Goals

The long-term vision for `diaryx_core`:

```
┌─────────────────────────────────────────────────────────────┐
│                     diaryx_core                             │
├─────────────────────────────────────────────────────────────┤
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐       │
│  │    Entry     │  │  Workspace   │  │   Search     │       │
│  │  Operations  │  │  Management  │  │   Engine     │       │
│  └──────────────┘  └──────────────┘  └──────────────┘       │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐       │
│  │   Template   │  │    Export    │  │   Publish    │       │
│  │    Engine    │  │   (Filter)   │  │   (HTML)     │       │
│  └──────────────┘  └──────────────┘  └──────────────┘       │
├─────────────────────────────────────────────────────────────┤
│  FileSystem Trait (RealFileSystem | InMemoryFileSystem)     │
└─────────────────────────────────────────────────────────────┘
           │                    │                    │
           ▼                    ▼                    ▼
    ┌──────────┐        ┌──────────────┐      ┌──────────┐
    │   CLI    │        │    WASM      │      │  Tauri   │
    │ (diaryx) │        │ (diaryx_wasm)│      │  Backend │
    └──────────┘        └──────────────┘      └──────────┘
```

All business logic should live in `diaryx_core`. Frontends should be thin wrappers that:

- Handle I/O (filesystem, user input, HTTP)
- Convert types for their environment
- Call core functions

---

Thank you for contributing to Diaryx! 🎉
