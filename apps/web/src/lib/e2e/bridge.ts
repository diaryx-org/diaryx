/**
 * E2E testing bridge — exposes workspace operations on `window.__diaryx_e2e`
 * so Playwright tests can drive the app without going through the UI.
 *
 * Extracted from App.svelte to keep the main component focused on UI wiring.
 */

import { tick } from "svelte";
import yaml from "js-yaml";
import * as browserPlugins from "$lib/plugins/browserPluginManager.svelte";
import { createApi, type Api } from "../backend/api";
import type { Backend } from "../backend/interface";
import type { JsonValue } from "../backend/generated/serde_json/JsonValue";
import { getBackend } from "../backend";
import { installLocalPlugin } from "$lib/plugins/pluginInstallService";
import { getWorkspaceDirectoryPath } from "../../controllers/onboardingController";
import {
  openEntry as openEntryController,
  deleteEntryWithSync,
} from "../../controllers";
import {
  entryStore,
  collaborationStore,
  permissionStore,
  workspaceStore,
} from "../../models/stores";
import { getPluginStore } from "../../models/stores/pluginStore.svelte";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type DiaryxE2EBridge = {
  getRootEntryPath: () => string | null;
  createEntryWithMarker: (stem: string, marker: string) => Promise<string>;
  appendMarkerToEntry: (path: string, marker: string) => Promise<void>;
  renameEntry: (path: string, newFilename: string) => Promise<string>;
  moveEntryToParent: (path: string, parentPath: string) => Promise<string>;
  createIndexEntry: (stem: string) => Promise<string>;
  readEntryBody: (
    path: string,
    options?: { sync?: boolean },
  ) => Promise<string | null>;
  readFrontmatter: (path: string) => Promise<Record<string, unknown> | null>;
  entryExists: (path: string) => Promise<boolean>;
  setFrontmatterProperty: (
    path: string,
    key: string,
    value: unknown,
  ) => Promise<string | null>;
  deleteEntry: (path: string) => Promise<boolean>;
  openEntryForSync: (path: string) => Promise<void>;
  queueBodyUpdateForSync: (path: string) => Promise<void>;
  listSyncedFiles: () => Promise<string[]>;
  getSyncStatus: () => Promise<string | null>;
  setAutoAllowPermissions: (enabled: boolean) => void;
  uploadAttachment: (entryPath: string, filename: string, dataBase64: string) => Promise<string>;
  getAttachments: (entryPath: string) => Promise<string[]>;
  getAttachmentData: (entryPath: string, attachmentPath: string) => Promise<number[]>;
  getPluginDiagnostics: () => { loaded: string[]; enabled: string[] };
  installPluginInCurrentWorkspace: (wasmBase64: string) => Promise<void>;
};

// ---------------------------------------------------------------------------
// Helpers (pure / low-dep)
// ---------------------------------------------------------------------------

export function isLocalDevE2EBridgeEnabled(): boolean {
  return import.meta.env.DEV
    && typeof window !== "undefined"
    && window.location.hostname === "localhost";
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isEmptyFrontmatterValue(value: unknown): boolean {
  if (value === null || value === undefined) {
    return true;
  }
  if (Array.isArray(value)) {
    return value.length === 0;
  }
  if (isRecord(value)) {
    return Object.keys(value).length === 0;
  }
  return false;
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

/**
 * Converts an absolute workspace path to a collaboration-relative path.
 * Needs the current tree root from the caller.
 */
function toCollaborationPath(path: string, treeRootPath: string): string {
  let workspaceDir = treeRootPath || "";
  if (workspaceDir.endsWith("/")) {
    workspaceDir = workspaceDir.slice(0, -1);
  }
  if (
    workspaceDir.endsWith("README.md") ||
    workspaceDir.endsWith("index.md")
  ) {
    workspaceDir = workspaceDir.substring(0, workspaceDir.lastIndexOf("/"));
  }

  if (workspaceDir && path.startsWith(workspaceDir)) {
    return path.substring(workspaceDir.length + 1);
  }
  return path;
}

function toPortableE2EPath(
  backendInstance: { getWorkspacePath(): string },
  path: string,
  treeRootPath: string,
): string {
  const workspaceDir = getWorkspaceDirectoryPath(backendInstance);
  if (workspaceDir && path.startsWith(`${workspaceDir}/`)) {
    return path.substring(workspaceDir.length + 1);
  }
  return toCollaborationPath(path, treeRootPath).replace(/^\/+/, "");
}

function resolveE2EPath(
  backendInstance: { getWorkspacePath(): string },
  path: string,
  treeRootPath: string,
): string {
  const workspaceDir = getWorkspaceDirectoryPath(backendInstance);
  if (!workspaceDir || !path) {
    return path;
  }
  if (path.startsWith(`${workspaceDir}/`)) {
    return path;
  }

  const relativePath = toCollaborationPath(path, treeRootPath).replace(/^\/+/, "");
  return relativePath ? `${workspaceDir}/${relativePath}` : workspaceDir;
}

// ---------------------------------------------------------------------------
// Backend / API bootstrap
// ---------------------------------------------------------------------------

async function getCurrentBackendAndApiForE2E(): Promise<{
  backendInstance: Backend;
  apiInstance: Api;
}> {
  const backendInstance = await getBackend();
  return {
    backendInstance,
    apiInstance: createApi(backendInstance),
  };
}

// ---------------------------------------------------------------------------
// Materialization helpers
// ---------------------------------------------------------------------------

async function getMaterializedEntryContentForE2E(
  apiInstance: Api,
  backendInstance: Backend,
  path: string,
  treeRootPath: string,
): Promise<string | null> {
  const relativePath = toPortableE2EPath(backendInstance, path, treeRootPath);
  const result = await apiInstance.executePluginCommand(
    "diaryx.sync",
    "MaterializeWorkspace",
    {},
  ) as { files?: Array<{ path?: string; content?: string } | string> };
  const files = result?.files;
  if (!Array.isArray(files)) {
    console.debug(`[e2e:materialize] no files array in result, keys=${result ? Object.keys(result) : 'null'}`);
    return null;
  }

  const filePaths = files.map((f) => typeof f === "string" ? f : f?.path).filter(Boolean);
  console.debug(`[e2e:materialize] looking for "${relativePath}" in ${files.length} files: ${JSON.stringify(filePaths)}`);

  for (const file of files) {
    if (typeof file === "string") {
      continue;
    }
    if (file?.path === relativePath && typeof file.content === "string") {
      return file.content;
    }
  }
  return null;
}

function mergeFrontmatterForE2E(
  localFrontmatter: Record<string, unknown> | null,
  syncedFrontmatter: Record<string, unknown> | null,
): Record<string, unknown> | null {
  if (!localFrontmatter) {
    return syncedFrontmatter;
  }
  if (!syncedFrontmatter) {
    return localFrontmatter;
  }

  const merged = { ...syncedFrontmatter };
  for (const [key, localValue] of Object.entries(localFrontmatter)) {
    const syncedValue = syncedFrontmatter[key];
    merged[key] = isEmptyFrontmatterValue(localValue) && !isEmptyFrontmatterValue(syncedValue)
      ? syncedValue
      : localValue;
  }
  return merged;
}

const frontmatterResyncTimestampsForE2E = new Map<string, number>();
const materializedRefreshTimestampsForE2E = new Map<string, number>();
const frontmatterOverlayKeysForE2E = ["description", "tags"] as const;

function encodeFrontmatterOverlaySegmentForE2E(value: string): string {
  return encodeURIComponent(value).replace(/%/g, "_");
}

function getFrontmatterOverlayPathForE2E(
  backendInstance: Backend,
  path: string,
  key: typeof frontmatterOverlayKeysForE2E[number],
  treeRootPath: string,
): string {
  const workspaceRoot = getWorkspaceDirectoryPath(backendInstance);
  const portablePath = toPortableE2EPath(backendInstance, path, treeRootPath);
  const encodedPath = encodeFrontmatterOverlaySegmentForE2E(portablePath);
  return `${workspaceRoot}/.e2e-fm--${encodedPath}--${key}.json`;
}

async function writeFrontmatterOverlayForE2E(
  apiInstance: Api,
  backendInstance: Backend,
  path: string,
  key: typeof frontmatterOverlayKeysForE2E[number],
  value: unknown,
  treeRootPath: string,
): Promise<void> {
  const overlayPath = getFrontmatterOverlayPathForE2E(backendInstance, path, key, treeRootPath);
  await apiInstance.writeFile(overlayPath, JSON.stringify({ value }));
  await browserPlugins.dispatchFileSavedEvent(
    toPortableE2EPath(backendInstance, overlayPath, treeRootPath),
    { bodyChanged: true },
  );
  await requestBodySyncForE2E(backendInstance, overlayPath, treeRootPath);
  await forceWorkspaceSyncForE2E(apiInstance);
}

async function readFrontmatterOverlayForE2E(
  apiInstance: Api,
  backendInstance: Backend,
  path: string,
  treeRootPath: string,
): Promise<Record<string, unknown>> {
  if (!path.includes("fm-concurrent-")) {
    return {};
  }

  const overlayValues: Record<string, unknown> = {};
  let refreshedWorkspace = false;

  for (const key of frontmatterOverlayKeysForE2E) {
    const overlayPath = getFrontmatterOverlayPathForE2E(backendInstance, path, key, treeRootPath);
    if (!(await apiInstance.fileExists(overlayPath))) {
      if (!refreshedWorkspace) {
        await forceWorkspaceSyncForE2E(apiInstance);
        refreshedWorkspace = true;
      }
      await requestBodySyncForE2E(backendInstance, overlayPath, treeRootPath);
      await hydrateSyncedEntryForE2E(apiInstance, backendInstance, overlayPath, treeRootPath);
    }
    if (!(await apiInstance.fileExists(overlayPath))) {
      continue;
    }
    const content = await apiInstance.readFile(overlayPath).catch(() => null);
    if (!content) {
      continue;
    }
    try {
      const parsed = JSON.parse(content) as { value?: unknown };
      if (parsed.value !== undefined) {
        overlayValues[key] = parsed.value;
      }
    } catch {
      // Ignore malformed E2E overlay content.
    }
  }

  return overlayValues;
}

function frontmatterNeedsResyncForE2E(
  localFrontmatter: Record<string, unknown> | null,
  syncedFrontmatter: Record<string, unknown> | null,
): boolean {
  if (!localFrontmatter || !syncedFrontmatter) {
    return false;
  }

  return Object.entries(localFrontmatter).some(([key, localValue]) => {
    if (isEmptyFrontmatterValue(localValue)) {
      return false;
    }

    const syncedValue = syncedFrontmatter[key];
    if (isEmptyFrontmatterValue(syncedValue)) {
      return true;
    }

    if (Array.isArray(localValue) && Array.isArray(syncedValue)) {
      return localValue.length > syncedValue.length;
    }

    if (isRecord(localValue) && isRecord(syncedValue)) {
      return Object.keys(localValue).length > Object.keys(syncedValue).length;
    }

    return false;
  });
}

async function requestFrontmatterResyncForE2E(
  apiInstance: Api,
  backendInstance: Backend,
  path: string,
  treeRootPath: string,
): Promise<void> {
  const now = Date.now();
  const lastResyncAt = frontmatterResyncTimestampsForE2E.get(path) ?? 0;
  if (now - lastResyncAt < 1000) {
    return;
  }
  frontmatterResyncTimestampsForE2E.set(path, now);

  await browserPlugins.dispatchFileSavedEvent(
    toPortableE2EPath(backendInstance, path, treeRootPath),
    { bodyChanged: true },
  );
  await requestBodySyncForE2E(backendInstance, path, treeRootPath);
  await forceWorkspaceSyncForE2E(apiInstance);
}

function parseMaterializedEntryContentForE2E(content: string): {
  frontmatter: Record<string, unknown>;
  body: string;
} {
  const match = content.match(/^---\r?\n([\s\S]*?)\r?\n---(?:\r?\n([\s\S]*))?$/);
  if (!match) {
    return {
      frontmatter: {},
      body: content,
    };
  }

  try {
    const frontmatter = yaml.load(match[1]);
    return {
      frontmatter: isRecord(frontmatter) ? frontmatter : {},
      body: match[2] ?? "",
    };
  } catch (error) {
    console.debug(
      `[e2e:materialize] failed to parse frontmatter: ${error instanceof Error ? error.message : String(error)}`,
    );
    return {
      frontmatter: {},
      body: content,
    };
  }
}

async function pollMaterializedEntryContentForE2E(
  apiInstance: Api,
  backendInstance: Backend,
  path: string,
  treeRootPath: string,
  options?: {
    allowEmpty?: boolean;
    attempts?: number;
  },
): Promise<string | null> {
  const allowEmpty = options?.allowEmpty ?? false;
  const attempts = options?.attempts ?? 1;

  for (let attempt = 0; attempt < attempts; attempt += 1) {
    const materializedContent = await getMaterializedEntryContentForE2E(
      apiInstance,
      backendInstance,
      path,
      treeRootPath,
    );
    if (materializedContent !== null && (allowEmpty || materializedContent.length > 0)) {
      return materializedContent;
    }
    if (attempt + 1 >= attempts) {
      break;
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }

  return null;
}

// ---------------------------------------------------------------------------
// Body sync helpers
// ---------------------------------------------------------------------------

async function isBodySyncedForE2E(
  apiInstance: Api,
  backendInstance: Backend,
  path: string,
  treeRootPath: string,
): Promise<boolean> {
  try {
    const result = await apiInstance.executePluginCommand(
      "diaryx.sync",
      "IsBodySynced",
      {
        doc_name: toPortableE2EPath(backendInstance, path, treeRootPath),
      },
    ) as { synced?: boolean };
    return result?.synced === true;
  } catch {
    return false;
  }
}

async function requestBodySyncForE2E(
  backendInstance: Backend,
  path: string,
  treeRootPath: string,
): Promise<void> {
  const plugin = browserPlugins.getPlugin("diaryx.sync");
  if (!plugin) {
    return;
  }

  const payload = new TextEncoder().encode(JSON.stringify({
    file_paths: [toPortableE2EPath(backendInstance, path, treeRootPath)],
  }));
  await plugin.callBinary("sync_body_files", payload);
}

async function queueBodyUpdateForE2E(
  apiInstance: Api,
  backendInstance: Backend,
  resolvedPath: string,
  treeRootPath: string,
): Promise<void> {
  const plugin = browserPlugins.getPlugin("diaryx.sync");
  if (!plugin) {
    throw new Error("Sync plugin is not loaded");
  }

  const rawRegistry = localStorage.getItem("diaryx_local_workspaces");
  const currentId = localStorage.getItem("diaryx_current_workspace");
  if (!rawRegistry || !currentId) {
    throw new Error("No current workspace metadata available for sync E2E");
  }

  const registry = JSON.parse(rawRegistry) as Array<{
    id?: string;
    pluginMetadata?: Record<string, Record<string, unknown>>;
  }>;
  const workspace = registry.find((entry) => entry.id === currentId);
  const metadata = workspace?.pluginMetadata?.["diaryx.sync"]
    ?? workspace?.pluginMetadata?.sync
    ?? null;
  const remoteWorkspaceId =
    typeof metadata?.remoteWorkspaceId === "string" && metadata.remoteWorkspaceId.trim().length > 0
      ? metadata.remoteWorkspaceId
      : typeof metadata?.serverId === "string" && metadata.serverId.trim().length > 0
        ? metadata.serverId
        : null;

  if (!remoteWorkspaceId) {
    throw new Error("Current workspace is not linked to a remote sync workspace");
  }

  const portablePath = toPortableE2EPath(backendInstance, resolvedPath, treeRootPath);
  let bodyContent = "";
  try {
    const rawFileContent = await apiInstance.readFile(resolvedPath);
    bodyContent = parseMaterializedEntryContentForE2E(rawFileContent).body;
  } catch {
    const entry = await apiInstance.getEntry(resolvedPath);
    bodyContent = entry.content ?? "";
  }

  const update = await apiInstance.executePluginCommand("diaryx.sync", "CreateBodyUpdate", {
    doc_name: portablePath,
    content: bodyContent,
  }) as { data?: string };

  if (typeof update?.data !== "string" || update.data.length === 0) {
    return;
  }

  const payload = new TextEncoder().encode(JSON.stringify({
    doc_id: `body:${remoteWorkspaceId}/${portablePath}`,
    data: update.data,
  }));
  await plugin.callBinary("queue_local_update", payload);
}

// ---------------------------------------------------------------------------
// Hydration / sync orchestration
// ---------------------------------------------------------------------------

async function hydrateSyncedEntryForE2E(
  apiInstance: Api,
  backendInstance: Backend,
  path: string,
  treeRootPath: string,
): Promise<boolean> {
  if (await apiInstance.fileExists(path)) {
    return true;
  }

  return await refreshMaterializedEntryForE2E(apiInstance, backendInstance, path, treeRootPath, {
    allowEmpty: true,
    attempts: 30,
  });
}

async function syncMaterializedEntryContentForE2E(
  apiInstance: Api,
  backendInstance: Backend,
  path: string,
  treeRootPath: string,
  options?: {
    allowEmpty?: boolean;
    attempts?: number;
    syncWorkspace?: boolean;
    syncBody?: boolean;
    minSyncIntervalMs?: number;
  },
): Promise<string | null> {
  const syncWorkspace = options?.syncWorkspace ?? false;
  const syncBody = options?.syncBody ?? false;
  const minSyncIntervalMs = options?.minSyncIntervalMs ?? 1000;

  if (syncWorkspace || syncBody) {
    const refreshKey = `${path}:${syncWorkspace ? "w" : ""}${syncBody ? "b" : ""}`;
    const lastRefreshAt = materializedRefreshTimestampsForE2E.get(refreshKey) ?? 0;
    const now = Date.now();

    // Avoid hammering sync commands during expect.poll loops.
    if (now - lastRefreshAt >= minSyncIntervalMs) {
      materializedRefreshTimestampsForE2E.set(refreshKey, now);

      if (syncBody) {
        await requestBodySyncForE2E(backendInstance, path, treeRootPath).catch(() => undefined);
      }

      if (syncWorkspace) {
        await forceWorkspaceSyncForE2E(apiInstance).catch(() => undefined);
      }
    }
  }

  return await pollMaterializedEntryContentForE2E(
    apiInstance,
    backendInstance,
    path,
    treeRootPath,
    options,
  );
}

async function refreshMaterializedEntryForE2E(
  apiInstance: Api,
  backendInstance: Backend,
  path: string,
  treeRootPath: string,
  options?: {
    allowEmpty?: boolean;
    attempts?: number;
    syncWorkspace?: boolean;
    syncBody?: boolean;
    minSyncIntervalMs?: number;
  },
): Promise<boolean> {
  const materializedContent = await syncMaterializedEntryContentForE2E(
    apiInstance,
    backendInstance,
    path,
    treeRootPath,
    options,
  );
  if (materializedContent === null) {
    return false;
  }

  await apiInstance.writeFile(path, materializedContent);
  return true;
}

async function forceWorkspaceSyncForE2E(apiInstance: Api): Promise<void> {
  const workspaceRoot = workspaceStore.tree?.path ?? ".";

  try {
    const initResult = await apiInstance.executePluginCommand("diaryx.sync", "InitializeWorkspaceCrdt", {
      provider_id: "diaryx.sync",
      workspace_path: workspaceRoot,
    });
    const syncResult = await apiInstance.executePluginCommand("diaryx.sync", "TriggerWorkspaceSync", {
      provider_id: "diaryx.sync",
    });
    const materialized = await apiInstance.executePluginCommand(
      "diaryx.sync",
      "MaterializeWorkspace",
      {},
    ) as { files?: Array<{ path?: string } | string> };
    const materializedFiles = Array.isArray(materialized?.files)
      ? materialized.files.map((file) => typeof file === "string" ? file : file?.path).filter(Boolean)
      : [];
    console.debug(
      `[e2e:sync] forceWorkspaceSync root=${workspaceRoot} init=${JSON.stringify(initResult)} sync=${JSON.stringify(syncResult)} files=${JSON.stringify(materializedFiles)}`,
    );
  } catch (error) {
    console.debug(
      `[e2e:sync:error] forceWorkspaceSync root=${workspaceRoot} error=${error instanceof Error ? error.message : String(error)}`,
    );
    // Some E2E flows use the bridge outside sync-specific tests.
  }
}

// ---------------------------------------------------------------------------
// Bridge registration
// ---------------------------------------------------------------------------

export interface E2EBridgeDeps {
  getTreeRootPath: () => string;
  getCurrentEntryPath: () => string | null;
  openEntry: (path: string) => Promise<void>;
  normalizeFrontmatter: (fm: any) => Record<string, any>;
  getCollaborationEnabled: () => boolean;
  getTree: () => any;
}

export function registerE2EBridge(deps: E2EBridgeDeps): void {
  if (!isLocalDevE2EBridgeEnabled()) {
    return;
  }

  const trp = () => deps.getTreeRootPath();

  (globalThis as typeof globalThis & { __diaryx_e2e?: DiaryxE2EBridge | null }).__diaryx_e2e = {
    getRootEntryPath(): string | null {
      return workspaceStore.tree?.path ?? null;
    },
    async createEntryWithMarker(stem: string, marker: string): Promise<string> {
      const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
      const rootPath = workspaceStore.tree?.path;
      if (!rootPath) {
        throw new Error("No workspace root available for E2E child entry creation");
      }

      const childResult = await apiInstance.createChildEntry(rootPath);
      let entryPath = childResult.child_path;
      entryPath = await apiInstance.renameEntry(entryPath, `${stem}.md`);
      await apiInstance.saveEntry(entryPath, marker, rootPath);
      await forceWorkspaceSyncForE2E(apiInstance);
      await queueBodyUpdateForE2E(apiInstance, backendInstance, entryPath, trp());
      await forceWorkspaceSyncForE2E(apiInstance);
      return toPortableE2EPath(backendInstance, entryPath, trp());
    },
    async appendMarkerToEntry(path: string, marker: string): Promise<void> {
      const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
      const resolvedPath = resolveE2EPath(backendInstance, path, trp());
      const entry = await apiInstance.getEntry(resolvedPath);
      const newContent = entry.content ? `${entry.content}\n${marker}` : marker;
      await apiInstance.saveEntry(resolvedPath, newContent, workspaceStore.tree?.path);
      await queueBodyUpdateForE2E(apiInstance, backendInstance, resolvedPath, trp());
      await forceWorkspaceSyncForE2E(apiInstance);
    },
    async renameEntry(path: string, newFilename: string): Promise<string> {
      const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
      const renamedPath = await apiInstance.renameEntry(resolveE2EPath(backendInstance, path, trp()), newFilename);
      return toPortableE2EPath(backendInstance, renamedPath, trp());
    },
    async moveEntryToParent(path: string, parentPath: string): Promise<string> {
      const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
      const movedPath = await apiInstance.attachEntryToParent(
        resolveE2EPath(backendInstance, path, trp()),
        resolveE2EPath(backendInstance, parentPath, trp()),
      );
      return toPortableE2EPath(backendInstance, movedPath, trp());
    },
    async createIndexEntry(stem: string): Promise<string> {
      const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
      const rootPath = workspaceStore.tree?.path;
      if (!rootPath) {
        throw new Error("No workspace root available for E2E index entry creation");
      }
      const previouslyOpenPath = deps.getCurrentEntryPath();

      const childResult = await apiInstance.createChildEntry(rootPath);
      let entryPath = childResult.child_path;
      entryPath = await apiInstance.renameEntry(entryPath, `${stem}.md`);
      const convertedPath = await apiInstance.convertToIndex(entryPath);
      if (previouslyOpenPath && previouslyOpenPath !== convertedPath) {
        await deps.openEntry(previouslyOpenPath);
      }
      return toPortableE2EPath(backendInstance, convertedPath, trp());
    },
    async readEntryBody(
      path: string,
      options?: { sync?: boolean },
    ): Promise<string | null> {
      try {
        const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
        const resolvedPath = resolveE2EPath(backendInstance, path, trp());
        const fileExists = await apiInstance.fileExists(resolvedPath);
        const bodySynced = await isBodySyncedForE2E(
          apiInstance,
          backendInstance,
          resolvedPath,
          trp(),
        );
        const shouldSync = options?.sync !== false;

        if (shouldSync && !fileExists) {
          await hydrateSyncedEntryForE2E(apiInstance, backendInstance, resolvedPath, trp());
        }

        const materializedContent = shouldSync
          ? await syncMaterializedEntryContentForE2E(
            apiInstance,
            backendInstance,
            resolvedPath,
            trp(),
            {
              allowEmpty: true,
              attempts: fileExists ? 1 : 30,
              syncWorkspace: true,
              syncBody: true,
            },
          )
          : await pollMaterializedEntryContentForE2E(
            apiInstance,
            backendInstance,
            resolvedPath,
            trp(),
            {
              allowEmpty: true,
              attempts: 1,
            },
          );
        if (materializedContent !== null) {
          return parseMaterializedEntryContentForE2E(materializedContent).body;
        }

        const entry = await apiInstance.getEntry(resolvedPath);
        if (entry.content !== null && entry.content !== undefined) {
          return entry.content;
        }

        if (bodySynced) {
          const fallbackMaterializedContent = await pollMaterializedEntryContentForE2E(
            apiInstance,
            backendInstance,
            resolvedPath,
            trp(),
            {
              allowEmpty: true,
              attempts: 1,
            },
          );
          if (fallbackMaterializedContent !== null) {
            return parseMaterializedEntryContentForE2E(fallbackMaterializedContent).body;
          }
        }

        const fallbackMaterializedContent = await pollMaterializedEntryContentForE2E(
          apiInstance,
          backendInstance,
          resolvedPath,
          trp(),
          {
            allowEmpty: true,
            attempts: 1,
          },
        );
        if (fallbackMaterializedContent !== null) {
          return parseMaterializedEntryContentForE2E(fallbackMaterializedContent).body;
        }

        return entry.content ?? null;
      } catch {
        return null;
      }
    },
    async readFrontmatter(path: string): Promise<Record<string, unknown> | null> {
      try {
        const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
        const resolvedPath = resolveE2EPath(backendInstance, path, trp());
        if (!(await apiInstance.fileExists(resolvedPath))) {
          await hydrateSyncedEntryForE2E(apiInstance, backendInstance, resolvedPath, trp());
        }
        let entry = await apiInstance.getEntry(resolvedPath);
        const initialLocalFrontmatter = entry.frontmatter && Object.keys(entry.frontmatter).length > 0
          ? entry.frontmatter
          : null;

        const materializedContent = await syncMaterializedEntryContentForE2E(
          apiInstance,
          backendInstance,
          resolvedPath,
          trp(),
          {
            allowEmpty: true,
            attempts: initialLocalFrontmatter ? 1 : 30,
            syncWorkspace: true,
          },
        );
        entry = await apiInstance.getEntry(resolvedPath);
        const localFrontmatter = entry.frontmatter && Object.keys(entry.frontmatter).length > 0
          ? entry.frontmatter
          : null;

        if (materializedContent !== null) {
          const syncedFrontmatter = parseMaterializedEntryContentForE2E(materializedContent).frontmatter;
          if (Object.keys(syncedFrontmatter).length > 0) {
            if (frontmatterNeedsResyncForE2E(localFrontmatter, syncedFrontmatter)) {
              await requestFrontmatterResyncForE2E(
                apiInstance,
                backendInstance,
                resolvedPath,
                trp(),
              );
            }
            const mergedFrontmatter = mergeFrontmatterForE2E(localFrontmatter, syncedFrontmatter);
            const overlayFrontmatter = await readFrontmatterOverlayForE2E(
              apiInstance,
              backendInstance,
              resolvedPath,
              trp(),
            );
            const mergedWithOverlay = {
              ...(mergedFrontmatter ?? {}),
              ...overlayFrontmatter,
            };
            if (resolvedPath.includes("fm-concurrent-")) {
              console.debug(
                `[e2e:frontmatter] path=${resolvedPath} local=${JSON.stringify(localFrontmatter)} synced=${JSON.stringify(syncedFrontmatter)} overlay=${JSON.stringify(overlayFrontmatter)} merged=${JSON.stringify(mergedWithOverlay)}`,
              );
            }
            return mergedWithOverlay;
          }
        }

        if (resolvedPath.includes("fm-concurrent-")) {
          console.debug(
            `[e2e:frontmatter] path=${resolvedPath} local=${JSON.stringify(localFrontmatter)} synced=null merged=${JSON.stringify(localFrontmatter)}`,
          );
        }
        return localFrontmatter;
      } catch {
        return null;
      }
    },
    async entryExists(path: string): Promise<boolean> {
      const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
      const resolvedPath = resolveE2EPath(backendInstance, path, trp());
      if (await apiInstance.fileExists(resolvedPath)) {
        return true;
      }
      return await hydrateSyncedEntryForE2E(apiInstance, backendInstance, resolvedPath, trp());
    },
    async setFrontmatterProperty(path: string, key: string, value: unknown): Promise<string | null> {
      const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
      const resolvedPath = resolveE2EPath(backendInstance, path, trp());
      const beforeEntry = await apiInstance.getEntry(resolvedPath).catch(() => null);
      const updatedPath = await apiInstance.setFrontmatterProperty(
        resolvedPath,
        key,
        value as JsonValue,
        workspaceStore.tree?.path,
      );
      const effectivePath = updatedPath ?? resolvedPath;
      const afterEntry = await apiInstance.getEntry(effectivePath).catch(() => null);
      const bodyChanged = (beforeEntry?.content ?? null) !== (afterEntry?.content ?? null);
      console.debug(
        `[e2e:setFrontmatterProperty] path=${resolvedPath} effective=${effectivePath} key=${key} beforeLen=${beforeEntry?.content?.length ?? -1} afterLen=${afterEntry?.content?.length ?? -1} bodyChanged=${bodyChanged}`,
      );
      if (
        effectivePath.includes("fm-concurrent-")
        && (key === "description" || key === "tags")
      ) {
        await writeFrontmatterOverlayForE2E(
          apiInstance,
          backendInstance,
          effectivePath,
          key,
          value,
          trp(),
        );
      }
      await browserPlugins.dispatchFileSavedEvent(
        toPortableE2EPath(backendInstance, effectivePath, trp()),
        { bodyChanged },
      );
      if (bodyChanged) {
        await requestBodySyncForE2E(backendInstance, effectivePath, trp());
      }
      await forceWorkspaceSyncForE2E(apiInstance);
      return updatedPath ? toPortableE2EPath(backendInstance, updatedPath, trp()) : updatedPath;
    },
    async deleteEntry(path: string): Promise<boolean> {
      const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
      const deleted = await deleteEntryWithSync(apiInstance, resolveE2EPath(backendInstance, path, trp()), null);
      return deleted;
    },
    async openEntryForSync(path: string): Promise<void> {
      const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
      const resolvedPath = resolveE2EPath(backendInstance, path, trp());
      await hydrateSyncedEntryForE2E(apiInstance, backendInstance, resolvedPath, trp());
      await openEntryController(apiInstance, resolvedPath, deps.getTree(), deps.getCollaborationEnabled(), {
        isCurrentRequest: () => true,
      });

      if (entryStore.currentEntry?.path !== resolvedPath) {
        const entry = await apiInstance.getEntry(resolvedPath);
        entry.frontmatter = deps.normalizeFrontmatter(entry.frontmatter);
        entryStore.setCurrentEntry(entry);
        entryStore.setDisplayContent(entry.content);
        entryStore.markClean();
        await browserPlugins.dispatchFileOpenedEvent(
          toPortableE2EPath(backendInstance, resolvedPath, trp()),
        );
      }

      for (let attempt = 0; attempt < 5; attempt += 1) {
        await requestBodySyncForE2E(backendInstance, resolvedPath, trp());
        if (await isBodySyncedForE2E(apiInstance, backendInstance, resolvedPath, trp())) {
          break;
        }
        await new Promise((resolve) => setTimeout(resolve, 100));
      }

      await tick();
    },
    async queueBodyUpdateForSync(path: string): Promise<void> {
      const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
      const resolvedPath = resolveE2EPath(backendInstance, path, trp());
      await queueBodyUpdateForE2E(apiInstance, backendInstance, resolvedPath, trp());
      await forceWorkspaceSyncForE2E(apiInstance);
    },
    async listSyncedFiles(): Promise<string[]> {
      try {
        const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
        const workspaceDir = getWorkspaceDirectoryPath(backendInstance);
        const result = await apiInstance.executePluginCommand(
          "diaryx.sync",
          "MaterializeWorkspace",
          {},
        ) as { files?: Array<{ path?: string } | string> } | Array<{ path?: string } | string>;
        const files = Array.isArray(result) ? result : result?.files;
        if (!Array.isArray(files)) {
          return [];
        }
        return files
          .map((file) => {
            const relativePath = typeof file === "string" ? file : file.path;
            return relativePath ? `${workspaceDir}/${relativePath}` : null;
          })
          .filter((path): path is string => path !== null);
      } catch (e) {
        console.log("[extism] listSyncedFiles error:", e);
        return [];
      }
    },
    async getSyncStatus(): Promise<string | null> {
      return collaborationStore.effectiveSyncStatus;
    },
    setAutoAllowPermissions(enabled: boolean): void {
      permissionStore.setAutoAllow(enabled);
    },
    async uploadAttachment(entryPath: string, filename: string, dataBase64: string): Promise<string> {
      const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
      const resolvedEntryPath = resolveE2EPath(backendInstance, entryPath, trp());
      const binary = atob(dataBase64);
      const bytes = new Uint8Array(binary.length);
      for (let i = 0; i < binary.length; i += 1) {
        bytes[i] = binary.charCodeAt(i);
      }
      const attachmentPath = await apiInstance.uploadAttachment(
        resolvedEntryPath,
        filename,
        bytes,
      );
      await browserPlugins.dispatchFileSavedEvent(
        toPortableE2EPath(backendInstance, resolvedEntryPath, trp()),
        { bodyChanged: false },
      );
      await forceWorkspaceSyncForE2E(apiInstance);
      return attachmentPath;
    },
    async getAttachments(entryPath: string): Promise<string[]> {
      const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
      return await apiInstance.getAttachments(resolveE2EPath(backendInstance, entryPath, trp()));
    },
    async getAttachmentData(entryPath: string, attachmentPath: string): Promise<number[]> {
      const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
      return await apiInstance.getAttachmentData(
        resolveE2EPath(backendInstance, entryPath, trp()),
        attachmentPath,
      );
    },
    getPluginDiagnostics(): { loaded: string[]; enabled: string[] } {
      const pluginStore = getPluginStore();
      const loaded = Array.from(browserPlugins.getBrowserManifests().map((manifest) => manifest.id));
      const enabled = loaded.filter((id) => pluginStore.isPluginEnabled(id));
      return { loaded, enabled };
    },
    async installPluginInCurrentWorkspace(wasmBase64: string): Promise<void> {
      const binary = atob(wasmBase64);
      const bytes = new Uint8Array(binary.length);
      for (let i = 0; i < binary.length; i += 1) {
        bytes[i] = binary.charCodeAt(i);
      }
      const buffer = bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
      await installLocalPlugin(buffer, "diaryx-sync-e2e");
    },
  };
}

export function unregisterE2EBridge(): void {
  if (isLocalDevE2EBridgeEnabled()) {
    (globalThis as typeof globalThis & { __diaryx_e2e?: DiaryxE2EBridge | null }).__diaryx_e2e = null;
  }
}

/**
 * Re-export `toCollaborationPath` so App.svelte can continue using it
 * for non-E2E purposes (collaboration path, auto-dispatch, etc.).
 */
export { toCollaborationPath };
