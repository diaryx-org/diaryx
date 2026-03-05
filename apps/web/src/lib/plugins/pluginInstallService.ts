import { createApi } from "$lib/backend/api";
import { getBackend, isTauri } from "$lib/backend";
import type { Backend } from "$lib/backend/interface";
import {
  inspectPluginWasm,
  installPlugin as browserInstallPlugin,
  uninstallPlugin as browserUninstallPlugin,
} from "$lib/plugins/browserPluginManager.svelte";
import type {
  PermissionType,
  PluginConfig,
  PluginPermissions,
} from "@/models/stores/permissionStore.svelte";
import type { RegistryPlugin } from "$lib/plugins/pluginRegistry";
import { workspaceStore } from "@/models/stores/workspaceStore.svelte";

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

async function platformInstall(
  bytes: ArrayBuffer,
  name?: string,
  expectedPluginId?: string,
): Promise<void> {
  if (isTauri()) {
    const backend: Backend = await getBackend();
    if (backend.installPlugin) {
      const manifestJson = await backend.installPlugin(new Uint8Array(bytes));
      if (expectedPluginId) {
        let installedId: string | null = null;
        try {
          const parsed = JSON.parse(manifestJson);
          if (typeof parsed?.id === "string") installedId = parsed.id;
        } catch {
          // Keep null to fail below.
        }

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
      return;
    }
  }

  await browserInstallPlugin(bytes, name);
}

export async function uninstallPlugin(pluginId: string): Promise<void> {
  if (isTauri()) {
    const backend: Backend = await getBackend();
    if (backend.uninstallPlugin) {
      await backend.uninstallPlugin(pluginId);
      return;
    }
  }

  await browserUninstallPlugin(pluginId);
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

async function persistDefaultPermissions(
  pluginId: string,
  defaults: PluginPermissions,
): Promise<void> {
  const rootIndexPath = workspaceStore.tree?.path;
  if (!rootIndexPath) return;

  const backend = await getBackend();
  const api = createApi(backend);
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

async function reviewAndInstall(
  bytes: ArrayBuffer,
  fallbackName?: string,
  expectedPluginId?: string,
): Promise<void> {
  if (isTauri()) {
    await platformInstall(bytes, fallbackName, expectedPluginId);
    return;
  }

  const inspected = await inspectPluginWasm(bytes);
  const pluginId = inspected.pluginId;
  if (expectedPluginId && pluginId !== expectedPluginId) {
    throw new Error(
      `Plugin ID mismatch: expected '${expectedPluginId}', got '${pluginId}'`,
    );
  }

  const pluginName = inspected.pluginName || fallbackName || pluginId;
  const requested = inspected.requestedPermissions;
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
    return;
  }

  if (requested?.defaults) {
    await persistDefaultPermissions(pluginId, requested.defaults);
  }

  await platformInstall(bytes, fallbackName ?? pluginName, expectedPluginId);
}

export async function installRegistryPlugin(
  plugin: RegistryPlugin,
): Promise<void> {
  const response = await fetch(plugin.artifact.url);
  if (!response.ok) {
    throw new Error(`Download failed: ${response.status}`);
  }

  const bytes = await response.arrayBuffer();
  await verifyRegistryArtifact(bytes, plugin.artifact.sha256);
  await reviewAndInstall(bytes, plugin.name, plugin.id);
}

export async function installLocalPlugin(
  bytes: ArrayBuffer,
  fallbackName?: string,
): Promise<void> {
  await reviewAndInstall(bytes, fallbackName);
}
