import type { Api } from "$lib/backend/api";
import {
  getPrimaryWorkspaceProviderLink,
  setPluginMetadata,
} from "$lib/storage/localWorkspaceRegistry.svelte";

interface BackendLike {
  getWorkspacePath?: () => string;
}

/**
 * Hydrate per-workspace provider links from the workspace root frontmatter.
 *
 * The sync plugin writes `plugins["diaryx.sync"].workspace_id` to the root
 * index frontmatter when a workspace is linked. That field is the cross-device
 * source of truth — but `WorkspaceProviderLink` records live in this device's
 * localStorage. When a workspace is linked from another device or via the CLI,
 * localStorage on this device is empty and the UI shows "local only" until we
 * read the frontmatter and seed the registry.
 *
 * This is best-effort: any failure (missing root, unreadable frontmatter,
 * fresh-but-empty workspace) is swallowed so it can never block a switch.
 */
export async function hydrateProviderLinksFromFrontmatter(
  workspaceId: string,
  api: Api,
  backend: BackendLike,
): Promise<void> {
  try {
    const workspacePath = backend.getWorkspacePath?.();
    if (!workspacePath) return;

    const rootIndex = await api.findRootIndex(workspacePath);
    if (!rootIndex) return;

    const fm = await api.getFrontmatter(rootIndex);
    const plugins = fm?.plugins;
    if (!plugins || typeof plugins !== "object" || Array.isArray(plugins)) return;

    for (const [pluginId, raw] of Object.entries(plugins as Record<string, unknown>)) {
      if (!raw || typeof raw !== "object" || Array.isArray(raw)) continue;
      const entry = raw as Record<string, unknown>;
      const remoteId = entry.workspace_id;
      if (typeof remoteId !== "string" || remoteId.trim().length === 0) continue;

      const existing = getPrimaryWorkspaceProviderLink(workspaceId);
      if (
        existing &&
        existing.pluginId === pluginId &&
        existing.remoteWorkspaceId === remoteId.trim()
      ) {
        continue;
      }

      setPluginMetadata(workspaceId, pluginId, {
        remoteWorkspaceId: remoteId.trim(),
        syncEnabled: true,
      });
    }
  } catch {
    // Best-effort hydration — never block a workspace switch on this.
  }
}
