import type { JsonValue } from "$lib/backend/generated/serde_json/JsonValue";
import {
  getCurrentWorkspaceId,
  getLocalWorkspace,
  getWorkspaceProviderLinks,
} from "$lib/storage/localWorkspaceRegistry.svelte";

type PluginCommandRunner = (
  pluginId: string,
  command: string,
  params?: JsonValue,
) => Promise<unknown>;

type WorkspaceRootResolver = {
  getWorkspacePath: () => string;
  resolveRootIndex?: (workspacePath: string) => Promise<string | null>;
};

function normalizeWorkspacePath(path: string | null | undefined): string | null {
  const trimmed = path?.trim();
  if (!trimmed) return null;
  return trimmed;
}

async function resolveCurrentWorkspaceRoot(
  backend: WorkspaceRootResolver,
): Promise<string | null> {
  const currentWorkspaceId = getCurrentWorkspaceId();
  const currentWorkspace = currentWorkspaceId
    ? getLocalWorkspace(currentWorkspaceId)
    : null;
  const registryPath = normalizeWorkspacePath(currentWorkspace?.path ?? null);
  if (registryPath) {
    return registryPath;
  }

  const workspacePath = normalizeWorkspacePath(backend.getWorkspacePath());
  if (!workspacePath) {
    return null;
  }

  try {
    return (await backend.resolveRootIndex?.(workspacePath)) ?? workspacePath;
  } catch {
    return workspacePath;
  }
}

export async function mirrorCurrentWorkspaceMutationToLinkedProviders(args: {
  backend: WorkspaceRootResolver;
  runPluginCommand: PluginCommandRunner;
}): Promise<void> {
  const currentWorkspaceId = getCurrentWorkspaceId();
  if (!currentWorkspaceId) {
    return;
  }

  const providerLinks = getWorkspaceProviderLinks(currentWorkspaceId).filter(
    (link) => link.syncEnabled,
  );
  if (providerLinks.length === 0) {
    return;
  }

  const workspaceRoot = await resolveCurrentWorkspaceRoot(args.backend);

  await Promise.allSettled(
    providerLinks.map(async (link) => {
      try {
        await args.runPluginCommand(link.pluginId, "InitializeWorkspaceCrdt", {
          provider_id: link.pluginId,
          ...(workspaceRoot ? { workspace_path: workspaceRoot } : {}),
        });
        await args.runPluginCommand(link.pluginId, "TriggerWorkspaceSync", {
          provider_id: link.pluginId,
        });
      } catch (error) {
        console.warn(
          `[browserWorkspaceMutationMirror] Failed to mirror workspace mutation for ${link.pluginId}:`,
          error,
        );
      }
    }),
  );
}
