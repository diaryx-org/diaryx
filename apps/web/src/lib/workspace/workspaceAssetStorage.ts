import { createApi, getBackend, getBackendSync } from "$lib/backend";
import type { TreeNode } from "$lib/backend";

const DIARYX_DIR = ".diaryx";
const PLUGINS_DIR = `${DIARYX_DIR}/plugins`;
const THEMES_DIR = `${DIARYX_DIR}/themes`;
const TYPOGRAPHIES_DIR = `${DIARYX_DIR}/typographies`;

function normalizePath(path: string): string {
  return path
    .replace(/\\/g, "/")
    .replace(/^\.\/+/, "")
    .replace(/^\/+/, "")
    .replace(/\/+/g, "/");
}

function joinPath(...parts: string[]): string {
  return normalizePath(parts.filter(Boolean).join("/"));
}

function isMissingFileError(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error);
  return (
    message.includes("NotFound") ||
    message.includes("not found") ||
    message.includes("could not be found") ||
    message.includes("object can not be found")
  );
}

function bytesToBase64Url(bytes: Uint8Array): string {
  let binary = "";
  for (let i = 0; i < bytes.length; i++) {
    binary += String.fromCharCode(bytes[i]);
  }
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/g, "");
}

function encodeStorageKey(key: string): string {
  return bytesToBase64Url(new TextEncoder().encode(key));
}

function collectLeafPaths(node: TreeNode, out: string[]): void {
  const children = Array.isArray(node.children) ? node.children : [];
  if (children.length === 0) {
    if (typeof node.path === "string" && node.path.length > 0) {
      out.push(normalizePath(node.path));
    }
    return;
  }

  for (const child of children) {
    collectLeafPaths(child, out);
  }
}

async function getTree(prefix: string): Promise<TreeNode | null> {
  try {
    const backend = await getBackend();
    return await createApi(backend).getFilesystemTree(prefix, true);
  } catch (error) {
    if (isMissingFileError(error)) {
      return null;
    }
    throw error;
  }
}

export function getPluginInstallPath(pluginId: string): string {
  return joinPath(PLUGINS_DIR, pluginId, "plugin.wasm");
}

export function getPluginStoragePath(pluginId: string, key: string): string {
  return joinPath(PLUGINS_DIR, pluginId, "state", `${encodeStorageKey(key)}.bin`);
}

export function getThemeSettingsPath(): string {
  return joinPath(THEMES_DIR, "settings.json");
}

export function getThemeModePath(): string {
  return joinPath(THEMES_DIR, "mode.json");
}

export function getThemeLibraryPath(): string {
  return joinPath(THEMES_DIR, "library.json");
}

export function getTypographySettingsPath(): string {
  return joinPath(TYPOGRAPHIES_DIR, "settings.json");
}

export function getTypographyLibraryPath(): string {
  return joinPath(TYPOGRAPHIES_DIR, "library.json");
}

export function tryGetWorkspaceBackendSync() {
  try {
    return getBackendSync();
  } catch {
    return null;
  }
}

export async function readWorkspaceText(path: string): Promise<string | null> {
  try {
    const backend = await getBackend();
    return await createApi(backend).readFile(normalizePath(path));
  } catch (error) {
    if (isMissingFileError(error)) {
      return null;
    }
    throw error;
  }
}

export async function writeWorkspaceText(path: string, content: string): Promise<void> {
  const backend = await getBackend();
  await createApi(backend).writeFile(normalizePath(path), content);
}

export async function readWorkspaceBinary(path: string): Promise<Uint8Array | null> {
  try {
    const backend = await getBackend();
    return await backend.readBinary(normalizePath(path));
  } catch (error) {
    if (isMissingFileError(error)) {
      return null;
    }
    throw error;
  }
}

export async function writeWorkspaceBinary(path: string, data: Uint8Array): Promise<void> {
  const backend = await getBackend();
  await backend.writeBinary(normalizePath(path), data);
}

export async function deleteWorkspacePath(path: string): Promise<void> {
  try {
    const backend = await getBackend();
    await createApi(backend).deleteFile(normalizePath(path));
  } catch (error) {
    if (!isMissingFileError(error)) {
      throw error;
    }
  }
}

export async function listWorkspaceFiles(prefix: string): Promise<string[]> {
  const normalizedPrefix = normalizePath(prefix);
  const tree = await getTree(normalizedPrefix);
  if (!tree) {
    return [];
  }

  const files: string[] = [];
  collectLeafPaths(tree, files);
  return files;
}

export async function deleteWorkspaceTree(prefix: string): Promise<void> {
  const files = await listWorkspaceFiles(prefix);
  await Promise.all(files.map((file) => deleteWorkspacePath(file)));
}
