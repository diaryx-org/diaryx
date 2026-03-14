import { createApi } from "$lib/backend/api";
import { getBackend, isTauri } from "$lib/backend";
import { proxyFetch } from "$lib/backend/proxyFetch";
import type { Backend } from "$lib/backend/interface";
import {
  inspectPluginWasm,
  installPlugin as browserInstallPlugin,
  uninstallPlugin as browserUninstallPlugin,
} from "$lib/plugins/browserPluginManager.svelte";
import {
  clearPreservedPluginEditorExtensions,
  preservePluginEditorExtensions,
} from "$lib/plugins/preservedEditorExtensions.svelte";
import {
  permissionStore,
  type PermissionType,
  type PluginConfig,
  type PluginPermissions,
} from "@/models/stores/permissionStore.svelte";
import { getPluginStore } from "@/models/stores/pluginStore.svelte";
import type { RegistryPlugin } from "$lib/plugins/pluginRegistry";
import { workspaceStore } from "@/models/stores/workspaceStore.svelte";
import {
  clearInstalledPluginSource,
  setInstalledPluginSource,
} from "$lib/plugins/pluginInstallSource.svelte";
import { mirrorCurrentWorkspaceMutationToLinkedProviders } from "$lib/sync/browserWorkspaceMutationMirror";

const PERMISSION_LABELS: Record<PermissionType, string> = {
  read_files: "Read files",
  edit_files: "Edit files",
  create_files: "Create files",
  delete_files: "Delete files",
  move_files: "Move files",
  http_requests: "HTTP requests",
  execute_commands: "Execute commands",
  plugin_storage: "Plugin storage",
};

function formatRuleSummary(
  permissionType: PermissionType,
  rule: { include: string[]; exclude: string[] },
): string {
  if (permissionType === "plugin_storage") return "all";
  if (!rule.include?.length) return "no includes";
  return rule.include.join(", ");
}

function hasConfiguredPermissions(config: PluginConfig | undefined): boolean {
  if (!config?.permissions) return false;
  return Object.values(config.permissions).some((rule) => rule != null);
}

function formatPluginInstallError(error: unknown): string {
  if (typeof error === "string") {
    return error;
  }

  if (error instanceof Error) {
    return error.message;
  }

  if (typeof error === "object" && error !== null) {
    const err = error as { message?: string; kind?: string; path?: string };
    if (typeof err.message === "string") {
      if (typeof err.path === "string" && typeof err.kind === "string") {
        return `${err.message} (${err.kind}: ${err.path})`;
      }
      if (typeof err.kind === "string") {
        return `${err.message} (${err.kind})`;
      }
      if (typeof err.path === "string") {
        return `${err.message} (${err.path})`;
      }
      return err.message;
    }

    try {
      return JSON.stringify(error);
    } catch {
      // Fall through to String(...) below.
    }
  }

  return String(error);
}

function buildPluginInstallError(
  stage: string,
  context: Record<string, unknown>,
  error: unknown,
): Error {
  const formattedError = formatPluginInstallError(error);
  console.error(`[pluginInstallService] ${stage} failed`, {
    ...context,
    error,
    formattedError,
  });
  return new Error(`${stage} failed: ${formattedError}`);
}

async function runPluginInstallStage<T>(
  stage: string,
  context: Record<string, unknown>,
  fn: () => Promise<T>,
): Promise<T> {
  console.info(`[pluginInstallService] ${stage}`, context);
  try {
    const result = await fn();
    console.info(`[pluginInstallService] ${stage} complete`, context);
    return result;
  } catch (error) {
    throw buildPluginInstallError(stage, context, error);
  }
}

async function refreshTauriPluginStore(backend: Backend): Promise<void> {
  const pluginStore = getPluginStore();
  await pluginStore.init(createApi(backend));
  await pluginStore.preloadInsertCommandIcons();
}

async function platformInstall(
  bytes: ArrayBuffer,
  name?: string,
  expectedPluginId?: string,
): Promise<string | null> {
  if (isTauri()) {
    const backend: Backend = await getBackend();
    if (backend.installPlugin) {
      const installContext = {
        expectedPluginId: expectedPluginId ?? null,
        fallbackName: name ?? null,
        bytes: bytes.byteLength,
      };
      const manifestJson = await runPluginInstallStage(
        "install plugin through Tauri backend",
        installContext,
        async () => await backend.installPlugin!(new Uint8Array(bytes)),
      );
      let installedId: string | null = null;
      try {
        const parsed = JSON.parse(manifestJson);
        if (typeof parsed?.id === "string") installedId = parsed.id;
      } catch {
        // Ignore parse errors; the mismatch check below will handle required IDs.
      }
      if (expectedPluginId) {
        if (!installedId) {
          throw new Error(
            "Installed plugin manifest did not include a valid plugin ID.",
          );
        }
        if (installedId !== expectedPluginId) {
          throw new Error(
            `Installed plugin ID mismatch: expected '${expectedPluginId}', got '${installedId}'`,
          );
        }
      }
      if (installedId) {
        clearPreservedPluginEditorExtensions(installedId);
      }
      await refreshTauriPluginStore(backend);
      return installedId ?? expectedPluginId ?? null;
    }
  }

  const manifest = await browserInstallPlugin(bytes, name);
  return String(manifest.id);
}

type PluginInstallInspection = {
  pluginId: string;
  pluginName: string;
  requestedPermissions?: PluginPermissionsManifest;
};

type PluginPermissionsManifest = {
  defaults?: PluginPermissions;
  reasons?: Partial<Record<PermissionType, string>>;
};

async function inspectPluginForInstall(
  bytes: ArrayBuffer,
): Promise<PluginInstallInspection> {
  if (isTauri()) {
    const backend: Backend = await getBackend();
    if (backend.inspectPlugin) {
      const inspected = await runPluginInstallStage(
        "inspect plugin manifest through Tauri backend",
        { bytes: bytes.byteLength },
        async () => await backend.inspectPlugin!(new Uint8Array(bytes)),
      );
      return {
        pluginId: inspected.pluginId,
        pluginName: inspected.pluginName,
        requestedPermissions:
          inspected.requestedPermissions as PluginPermissionsManifest | undefined,
      };
    }
  }

  return await inspectPluginWasm(bytes);
}

export async function uninstallPlugin(pluginId: string): Promise<void> {
  if (isTauri()) {
    const backend: Backend = await getBackend();
    if (backend.uninstallPlugin) {
      const removedManifest =
        getPluginStore().allManifests.find(
          (manifest) => String(manifest.id) === pluginId,
        ) ?? null;
      await backend.uninstallPlugin(pluginId);
      clearInstalledPluginSource(pluginId);
      getPluginStore().clearPluginEnabled(pluginId);
      preservePluginEditorExtensions(removedManifest);
      await refreshTauriPluginStore(backend);
      return;
    }
  }

  await browserUninstallPlugin(pluginId);
  clearInstalledPluginSource(pluginId);
}

function normalizeSha256(value: string): string {
  return value.trim().toLowerCase().replace(/^sha256:/, "");
}

async function sha256Hex(bytes: ArrayBuffer): Promise<string> {
  if (typeof crypto === "undefined" || !crypto.subtle) {
    throw new Error("SHA-256 verification is unavailable in this runtime.");
  }

  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return Array.from(new Uint8Array(digest))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

export async function verifyRegistryArtifact(
  bytes: ArrayBuffer,
  expectedSha: string,
): Promise<void> {
  const actual = await sha256Hex(bytes);
  if (actual !== normalizeSha256(expectedSha)) {
    throw new Error("Plugin integrity check failed (SHA-256 mismatch)");
  }
}

async function getWorkspaceRootIndexContext(): Promise<{
  api: ReturnType<typeof createApi>;
  rootIndexPath: string | null;
}> {
  const backend = await getBackend();
  const api = createApi(backend);
  const rootIndexPath = await api.resolveWorkspaceRootIndexPath(
    workspaceStore.tree?.path,
  );
  return { api, rootIndexPath };
}

async function persistDefaultPermissions(
  pluginId: string,
  defaults: PluginPermissions,
): Promise<void> {
  const { api, rootIndexPath } = await getWorkspaceRootIndexContext();
  if (!rootIndexPath) return;
  const fm = await api.getFrontmatter(rootIndexPath);
  const existingPlugins =
    (fm.plugins as Record<string, PluginConfig> | undefined) ?? {};
  const existingPluginConfig = existingPlugins[pluginId] ?? { permissions: {} };
  const mergedPermissions: PluginPermissions = {
    ...(existingPluginConfig.permissions ?? {}),
  };

  for (const [permissionType, requestedRule] of Object.entries(defaults)) {
    if (!requestedRule) continue;
    if (!mergedPermissions[permissionType as PermissionType]) {
      mergedPermissions[permissionType as PermissionType] = {
        include: [...(requestedRule.include ?? [])],
        exclude: [...(requestedRule.exclude ?? [])],
      };
    }
  }

  const nextPlugins: Record<string, PluginConfig> = {
    ...existingPlugins,
    [pluginId]: {
      ...existingPluginConfig,
      permissions: mergedPermissions,
    },
  };

  await api.setFrontmatterProperty(
    rootIndexPath,
    "plugins",
    nextPlugins as any,
    rootIndexPath,
  );
}

/**
 * Check whether a plugin already has permissions configured.
 *
 * Reads from the in-memory plugins config (set via permissionStore persistence
 * handlers) rather than re-reading frontmatter from the backend, which avoids
 * stale-cache issues during onboarding when permissions were just pre-persisted.
 */
function hasExistingPermissions(pluginId: string): boolean {
  const pluginsConfig = permissionStore.getPluginsConfig();
  if (!pluginsConfig) return false;

  return hasConfiguredPermissions(pluginsConfig[pluginId]);
}

async function hasPersistedPermissions(pluginId: string): Promise<boolean> {
  const { api, rootIndexPath } = await getWorkspaceRootIndexContext();
  if (!rootIndexPath) return false;

  try {
    const fm = await api.getFrontmatter(rootIndexPath);
    const pluginsConfig = fm.plugins as Record<string, PluginConfig> | undefined;
    return hasConfiguredPermissions(pluginsConfig?.[pluginId]);
  } catch {
    return false;
  }
}

async function hasExistingOrPersistedPermissions(pluginId: string): Promise<boolean> {
  if (hasExistingPermissions(pluginId)) {
    return true;
  }

  return await hasPersistedPermissions(pluginId);
}

async function reviewAndInstall(
  bytes: ArrayBuffer,
  fallbackName?: string,
  expectedPluginId?: string,
): Promise<string | null> {
  const inspected = await inspectPluginForInstall(bytes);
  const pluginId = inspected.pluginId;
  if (expectedPluginId && pluginId !== expectedPluginId) {
    throw new Error(
      `Plugin ID mismatch: expected '${expectedPluginId}', got '${pluginId}'`,
    );
  }

  const pluginName = inspected.pluginName || fallbackName || pluginId;
  const requested = inspected.requestedPermissions;
  const installContext = {
    pluginId,
    pluginName,
    expectedPluginId: expectedPluginId ?? null,
    fallbackName: fallbackName ?? null,
    bytes: bytes.byteLength,
  };
  console.info("[pluginInstallService] Reviewed plugin manifest", {
    ...installContext,
    requestedPermissionTypes: Object.keys(requested?.defaults ?? {}),
  });

  // Skip the review dialog if permissions are already configured (e.g. from
  // a starter workspace with pre-set permissions during onboarding).
  const alreadyConfigured = await hasExistingOrPersistedPermissions(pluginId);
  if (alreadyConfigured) {
    console.info("[pluginInstallService] Permissions already configured; skipping review", {
      pluginId,
      pluginName,
    });
    return await platformInstall(bytes, fallbackName ?? pluginName, expectedPluginId);
  }

  const defaults = requested?.defaults ?? {};
  const reasons = requested?.reasons ?? {};

  const requestedLines = Object.entries(defaults)
    .filter(([, rule]) => !!rule)
    .map(([permissionType, rule]) => {
      const typed = permissionType as PermissionType;
      const reason = reasons[typed];
      const summary = formatRuleSummary(typed, rule!);
      if (reason) {
        return `- ${PERMISSION_LABELS[typed]}: ${summary}\n  Why: ${reason}`;
      }
      return `- ${PERMISSION_LABELS[typed]}: ${summary}`;
    });

  const details =
    requestedLines.length > 0
      ? requestedLines.join("\n")
      : "- This plugin requests no default permissions.";

  const approved = window.confirm(
    `Install "${pluginName}" (${pluginId})?\n\n` +
      `Requested default permissions:\n${details}\n\n` +
      `Approved defaults will be saved in root frontmatter under plugins.${pluginId}.permissions.`,
  );

  if (!approved) {
    console.info("[pluginInstallService] Plugin install cancelled during permission review", {
      pluginId,
      pluginName,
    });
    return null;
  }

  if (requested?.defaults) {
    await runPluginInstallStage(
      "persist plugin permission defaults",
      installContext,
      async () => await persistDefaultPermissions(pluginId, requested.defaults!),
    );
  }

  const installedPluginId = await platformInstall(
    bytes,
    fallbackName ?? pluginName,
    expectedPluginId,
  );
  console.info("[pluginInstallService] Plugin install finished", {
    ...installContext,
    installedPluginId: installedPluginId ?? pluginId,
  });
  return installedPluginId;
}

async function bootstrapLinkedWorkspaceSyncState(): Promise<void> {
  if (isTauri()) {
    return;
  }

  const backend = await getBackend();
  const api = createApi(backend);

  await mirrorCurrentWorkspaceMutationToLinkedProviders({
    backend: {
      getWorkspacePath: () => backend.getWorkspacePath(),
      resolveRootIndex: async (workspacePath) => {
        const finder = (backend as { findRootIndex?: (path: string) => Promise<string> }).findRootIndex;
        return typeof finder === "function" ? await finder(workspacePath) : workspacePath;
      },
    },
    runPluginCommand: async (pluginId, command, params = null) =>
      await api.executePluginCommand(pluginId, command, params),
  });
}

export async function installRegistryPlugin(
  plugin: RegistryPlugin,
): Promise<void> {
  const installContext = {
    pluginId: plugin.id,
    pluginName: plugin.name,
    version: plugin.version,
    artifactUrl: plugin.artifact.url,
  };
  console.info("[pluginInstallService] Installing registry plugin", installContext);
  const response = await runPluginInstallStage(
    "download registry plugin artifact",
    installContext,
    async () => await proxyFetch(plugin.artifact.url),
  );
  if (!response.ok) {
    throw buildPluginInstallError("download registry plugin artifact", installContext, {
      message: `Download failed: ${response.status}`,
      kind: "HttpError",
    });
  }

  const bytes = await runPluginInstallStage(
    "read downloaded registry plugin bytes",
    installContext,
    async () => await response.arrayBuffer(),
  );
  console.info("[pluginInstallService] Downloaded registry plugin bytes", {
    ...installContext,
    bytes: bytes.byteLength,
  });
  await runPluginInstallStage(
    "verify registry plugin artifact integrity",
    {
      ...installContext,
      bytes: bytes.byteLength,
      sha256: plugin.artifact.sha256,
    },
    async () => await verifyRegistryArtifact(bytes, plugin.artifact.sha256),
  );
  const installedPluginId = await reviewAndInstall(bytes, plugin.name, plugin.id);
  setInstalledPluginSource(installedPluginId ?? plugin.id, "registry");
  await bootstrapLinkedWorkspaceSyncState().catch((error) => {
    console.warn("[pluginInstallService] Failed to bootstrap linked workspace sync state:", error);
  });
}

export async function installLocalPlugin(
  bytes: ArrayBuffer,
  fallbackName?: string,
): Promise<void> {
  console.info("[pluginInstallService] Installing local plugin bytes", {
    fallbackName: fallbackName ?? null,
    bytes: bytes.byteLength,
  });
  const installedPluginId = await reviewAndInstall(bytes, fallbackName);
  if (installedPluginId) {
    setInstalledPluginSource(installedPluginId, "local");
  }
  await bootstrapLinkedWorkspaceSyncState().catch((error) => {
    console.warn("[pluginInstallService] Failed to bootstrap linked workspace sync state:", error);
  });
}
