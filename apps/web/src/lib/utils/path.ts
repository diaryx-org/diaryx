/**
 * Workspace path helpers (TypeScript mirror of `diaryx_core::path_utils`).
 *
 * All helpers are pure string transforms — no filesystem access, no backend
 * calls. They consolidate previously scattered copies from `lib/backend/api.ts`,
 * `lib/workspace/rootPath.ts`, `controllers/workspaceController.ts`, and
 * `controllers/entryController.ts` so there is ONE canonical implementation.
 *
 * ## Scope
 *
 * These helpers are intentionally sync TS so they can be used in Svelte
 * template expressions and reactive derivations. The canonical Rust
 * implementations live in `diaryx_core::path_utils` and
 * `diaryx_core::workspace` — every behavior here should match Rust byte for
 * byte, and a parity test should be added when Rust exposes the equivalent
 * via `Command` or `#[wasm_bindgen]`.
 *
 * For path operations that require workspace context (root discovery,
 * canonicalization across files, link-format aware resolution), route
 * through `api.findRootIndex()` / `api.resolveWorkspaceRootIndexPath()` /
 * `api.canonicalizeLink()` instead — those live in Rust.
 */

import type { Api } from '$lib/backend';
import type { TreeNode } from '$lib/backend/generated';

// ---------------------------------------------------------------------------
// Basic string normalization
// ---------------------------------------------------------------------------

/** Convert backslashes to forward slashes. */
export function normalizeSlashes(value: string): string {
  return value.replace(/\\/g, '/');
}

/**
 * Normalize a display-oriented tree path: forward slashes, strip leading
 * `./`. Preserves leading `/` so absolute Tauri paths survive.
 *
 * Use this for paths that flow into UI state (the workspace tree, store keys,
 * route params). For CRDT/sync keys where `README.md` and `/README.md` must
 * collide, use {@link normalizeSyncPath} instead.
 */
export function normalizeTreePath(path: string): string {
  return path.replace(/\\/g, '/').replace(/^\.\/+/, '');
}

/**
 * Normalize a workspace-relative sync path for CRDT keying: forward slashes,
 * strip leading `./`, strip leading `/`.
 *
 * Mirror of `diaryx_core::path_utils::normalize_sync_path`.
 */
export function normalizeSyncPath(path: string): string {
  return path.replace(/\\/g, '/').replace(/^\.\/+/, '').replace(/^\/+/, '');
}

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

/** Whether the final segment ends in `.md` (case-insensitive). */
export function isMarkdownPath(path: string): boolean {
  return /\.md$/i.test(path);
}

/** Whether the final segment ends in `.md` or `.markdown` (case-insensitive). */
export function isMarkdownTreePath(path: string | null | undefined): path is string {
  if (!path) return false;
  const lastSegment = path.split('/').pop() ?? '';
  return /\.(md|markdown)$/i.test(lastSegment);
}

/** Whether the path ends in `README.md` or `index.md` (a root-index file). */
export function isRootIndexPath(path: string): boolean {
  return /(^|\/)(README|index)\.md$/.test(path);
}

// ---------------------------------------------------------------------------
// Path decomposition
// ---------------------------------------------------------------------------

/** Parent directory path, or `null` if there is none (no slash or leading slash only). */
export function parentDirectory(path: string): string | null {
  const slash = path.lastIndexOf('/');
  return slash > 0 ? path.slice(0, slash) : null;
}

/**
 * Get the workspace directory path given either the workspace dir or a
 * root-index file path (`…/README.md`, `…/index.md`, or any `…/*.md`).
 *
 * - Returns the path unchanged if it is already a directory.
 * - Returns the parent directory if it is a markdown file.
 * - Returns `"."` when given a bare markdown filename with no parent.
 */
export function getWorkspaceDirectoryPath(path: string): string {
  const normalizedPath = path.replace(/\\/g, '/').replace(/\/$/, '');
  if (!normalizedPath) {
    return path;
  }

  const lastSegment = normalizedPath.split('/').pop() ?? '';
  const isMarkdown = /\.(md|markdown)$/i.test(lastSegment);
  if (!isMarkdown) {
    return normalizedPath;
  }

  const lastSlash = normalizedPath.lastIndexOf('/');
  if (lastSlash < 0) {
    return '.';
  }

  return normalizedPath.slice(0, lastSlash) || '/';
}

// ---------------------------------------------------------------------------
// Workspace root normalization
// ---------------------------------------------------------------------------

/**
 * Normalize a workspace path (which may be a root-index file path) to the
 * owning workspace directory. Returns `null` for empty input.
 *
 * Unlike {@link getWorkspaceDirectoryPath}, this only strips the *known*
 * root-index filenames (`README.md` / `index.md`) — other markdown files
 * are returned as-is (they are not considered workspace roots).
 */
export function normalizeWorkspaceRootPath(
  workspacePath: string | null | undefined,
): string | null {
  if (!workspacePath) return null;

  const normalized = normalizeSlashes(workspacePath).replace(/\/+$/, '');
  if (!normalized) return null;

  if (normalized.endsWith('/README.md') || normalized.endsWith('/index.md')) {
    const slash = normalized.lastIndexOf('/');
    return slash > 0 ? normalized.slice(0, slash) : null;
  }

  return normalized;
}

/**
 * Light normalization of a candidate workspace path: trim trailing slashes,
 * unify slashes. Returns `null` for empty input.
 */
export function normalizeWorkspacePathCandidate(
  path: string | null | undefined,
): string | null {
  if (!path) return null;

  const normalized = normalizeSlashes(path).replace(/\/+$/, '');
  return normalized || null;
}

// ---------------------------------------------------------------------------
// Workspace-relative transforms
// ---------------------------------------------------------------------------

/**
 * Strip the workspace root prefix from a path, returning the workspace-relative
 * path. The workspace directory is derived from `workspacePath` (which may be
 * a root-index file path).
 *
 * If the path is outside the workspace, returns the path with forward slashes
 * and no leading slash.
 *
 * Mirror of `diaryx_core::path_utils::strip_workspace_root_prefix`
 * (simplified for the web-host case where workspace paths are already
 * virtual/relative).
 */
export function toWorkspaceRelativePath(workspacePath: string, path: string): string {
  const workspaceDir = getWorkspaceDirectoryPath(workspacePath);
  const normalizedPath = normalizeSlashes(path);

  if (workspaceDir && workspaceDir !== '.' && normalizedPath.startsWith(`${workspaceDir}/`)) {
    return normalizedPath.slice(workspaceDir.length + 1);
  }

  return normalizedPath.replace(/^\/+/, '');
}

// ---------------------------------------------------------------------------
// Workspace validation root resolution
// ---------------------------------------------------------------------------

/**
 * Resolve the root-index file path used for workspace-scoped operations
 * (validation, convert-links, etc.).
 *
 * Prefers the already-loaded tree's path, then falls back to asking the
 * backend via `api.resolveWorkspaceRootIndexPath`.
 */
export async function resolveWorkspaceValidationRootPath(
  api: Api,
  tree: TreeNode | null,
  backendWorkspacePath: string,
): Promise<string> {
  if (tree?.path) {
    return tree.path;
  }

  return (
    (await api.resolveWorkspaceRootIndexPath(backendWorkspacePath)) ??
    backendWorkspacePath
  );
}
