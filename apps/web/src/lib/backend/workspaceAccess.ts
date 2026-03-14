import { isTauri } from "./interface";

function firstSelectedFolder(
  selection: string | string[] | null,
): string | null {
  if (typeof selection === "string") return selection;
  if (Array.isArray(selection)) {
    return selection.find((value) => typeof value === "string") ?? null;
  }
  return null;
}

/**
 * Convert a user-selected workspace folder into a sandbox-safe native path.
 *
 * On macOS App Store/TestFlight builds this persists a security-scoped
 * bookmark so later workspace switches can restore access instead of reusing
 * a raw filesystem path with no bookmark backing it.
 */
export async function authorizeWorkspacePath(path: string): Promise<string> {
  const trimmed = path.trim();
  if (!trimmed || !isTauri()) return trimmed;

  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<string>("authorize_workspace_path", {
    workspacePath: trimmed,
  });
}

/**
 * Open a native folder picker and immediately authorize the selected path.
 */
export async function pickAuthorizedWorkspaceFolder(
  title: string,
): Promise<string | null> {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selection = await open({ directory: true, title });
  const folder = firstSelectedFolder(selection);
  if (!folder) return null;
  return await authorizeWorkspacePath(folder);
}
