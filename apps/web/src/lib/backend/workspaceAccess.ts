import { isTauri } from "./interface";
import { isIOS } from "$lib/hooks/useMobile.svelte";

function firstSelectedPath(
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
  if (isTauri() && isIOS()) {
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke<string | null>("pick_authorized_workspace_folder", {
      title,
    });
  }

  const { open } = await import("@tauri-apps/plugin-dialog");
  const selection = await open({ directory: true, title });
  const folder = firstSelectedPath(selection);
  if (!folder) return null;
  return await authorizeWorkspacePath(folder);
}

/**
 * Open a native file picker and immediately authorize the selected path.
 *
 * This is primarily an iOS fallback for providers that allow opening a single
 * file but do not allow picking a whole folder.
 */
export async function pickAuthorizedWorkspaceFile(
  title: string,
): Promise<string | null> {
  if (isTauri() && isIOS()) {
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke<string | null>("pick_authorized_workspace_file", {
      title,
    });
  }

  const { open } = await import("@tauri-apps/plugin-dialog");
  const selection = await open({
    title,
    filters: [{ name: "Markdown", extensions: ["md", "markdown", "txt"] }],
  });
  const file = firstSelectedPath(selection);
  if (!file) return null;
  return await authorizeWorkspacePath(file);
}
