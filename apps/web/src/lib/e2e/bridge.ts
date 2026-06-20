/**
 * E2E testing bridge — exposes workspace operations on `window.__diaryx_e2e`
 * so Playwright tests can drive the app without going through the UI.
 *
 * Extracted from App.svelte to keep the main component focused on UI wiring.
 */

import * as browserPlugins from "$lib/plugins/browserPluginManager.svelte";
import { createApi, type Api } from "../backend/api";
import type { Backend } from "../backend/interface";
import type { YamlValue } from "../backend/generated/YamlValue";
import { getBackend } from "../backend";
import { getWorkspaceDirectoryPath } from "../../controllers/onboardingController";
import { deleteEntry } from "../../controllers";
import {
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
  readEntryBody: (path: string) => Promise<string | null>;
  readFrontmatter: (path: string) => Promise<Record<string, unknown> | null>;
  entryExists: (path: string) => Promise<boolean>;
  setFrontmatterProperty: (
    path: string,
    key: string,
    value: unknown,
  ) => Promise<string | null>;
  deleteEntry: (path: string) => Promise<boolean>;
  setAutoAllowPermissions: (enabled: boolean) => void;
  uploadAttachment: (entryPath: string, filename: string, dataBase64: string) => Promise<string>;
  getAttachments: (entryPath: string) => Promise<string[]>;
  getAttachmentData: (entryPath: string, attachmentPath: string) => Promise<number[]>;
  getPluginDiagnostics: () => { loaded: string[]; enabled: string[] };
};

// ---------------------------------------------------------------------------
// Helpers (pure / low-dep)
// ---------------------------------------------------------------------------

export function isLocalDevE2EBridgeEnabled(): boolean {
  return import.meta.env.DEV
    && typeof window !== "undefined"
    && window.location.hostname === "localhost";
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

      await browserPlugins.dispatchFileSavedEvent(
        toPortableE2EPath(backendInstance, entryPath, trp()),
        { bodyChanged: true },
      );

      return toPortableE2EPath(backendInstance, entryPath, trp());
    },
    async appendMarkerToEntry(path: string, marker: string): Promise<void> {
      const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
      const resolvedPath = resolveE2EPath(backendInstance, path, trp());
      const entry = await apiInstance.getEntry(resolvedPath);
      const newContent = entry.content ? `${entry.content}\n${marker}` : marker;
      await apiInstance.saveEntry(resolvedPath, newContent, workspaceStore.tree?.path);

      await browserPlugins.dispatchFileSavedEvent(
        toPortableE2EPath(backendInstance, resolvedPath, trp()),
        { bodyChanged: true },
      );
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
    async readEntryBody(path: string): Promise<string | null> {
      try {
        const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
        const resolvedPath = resolveE2EPath(backendInstance, path, trp());

        const entry = await apiInstance.getEntry(resolvedPath);
        return entry.content ?? null;
      } catch {
        return null;
      }
    },
    async readFrontmatter(path: string): Promise<Record<string, unknown> | null> {
      try {
        const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
        const resolvedPath = resolveE2EPath(backendInstance, path, trp());

        const entry = await apiInstance.getEntry(resolvedPath);
        const fm = entry.frontmatter;
        return fm && Object.keys(fm).length > 0 ? fm : null;
      } catch {
        return null;
      }
    },
    async entryExists(path: string): Promise<boolean> {
      const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
      const resolvedPath = resolveE2EPath(backendInstance, path, trp());
      return await apiInstance.fileExists(resolvedPath);
    },
    async setFrontmatterProperty(path: string, key: string, value: unknown): Promise<string | null> {
      const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
      const resolvedPath = resolveE2EPath(backendInstance, path, trp());
      const updatedPath = await apiInstance.setFrontmatterProperty(
        resolvedPath,
        key,
        value as YamlValue,
        workspaceStore.tree?.path,
      );
      const effectivePath = updatedPath ?? resolvedPath;

      await browserPlugins.dispatchFileSavedEvent(
        toPortableE2EPath(backendInstance, effectivePath, trp()),
        { bodyChanged: true },
      );

      return updatedPath ? toPortableE2EPath(backendInstance, updatedPath, trp()) : updatedPath;
    },
    async deleteEntry(path: string): Promise<boolean> {
      const { backendInstance, apiInstance } = await getCurrentBackendAndApiForE2E();
      return await deleteEntry(apiInstance, resolveE2EPath(backendInstance, path, trp()), null);
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
