/**
 * Creates JsAsyncFileSystem callbacks that dispatch to a storage Extism plugin.
 *
 * This bridges the gap between Extism plugin commands (ReadFile, WriteFile, etc.)
 * and the JsAsyncFileSystem callback interface used by DiaryxBackend.createFromJsFileSystem().
 *
 * Usage:
 *   const callbacks = createPluginFileSystemCallbacks("diaryx.storage.s3");
 *   const backend = await DiaryxBackend.createFromJsFileSystem(callbacks);
 */

import type { JsFileSystemCallbacks } from "$lib/wasm/diaryx_wasm";

async function pluginCmd(
  pluginId: string,
  command: string,
  params: Record<string, unknown>,
): Promise<Record<string, unknown>> {
  const { dispatchCommand } = await import(
    "$lib/plugins/browserPluginManager.svelte"
  );
  const result = await dispatchCommand(pluginId, command, params);
  if (!result.success) {
    throw new Error(
      result.error ?? `Plugin command ${command} failed`,
    );
  }
  return (result.data as Record<string, unknown>) ?? {};
}

function bytesToBase64(data: Uint8Array): string {
  let binary = "";
  for (let i = 0; i < data.length; i++) {
    binary += String.fromCharCode(data[i]);
  }
  return btoa(binary);
}

function base64ToBytes(b64: string): Uint8Array {
  const binary = atob(b64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

export function createPluginFileSystemCallbacks(
  pluginId: string,
): JsFileSystemCallbacks {
  return {
    read: async (path: string): Promise<Uint8Array> => {
      const result = await pluginCmd(pluginId, "ReadBinary", { path });
      return base64ToBytes(result.data as string);
    },

    readToString: async (path: string): Promise<string> => {
      const result = await pluginCmd(pluginId, "ReadFile", { path });
      return result.content as string;
    },

    readDir: async (
      path: string,
    ): Promise<Array<string | { name: string; kind: "file" | "dir" | "symlink" }>> => {
      const result = await pluginCmd(pluginId, "ListFiles", { dir: path });
      const files = (result.files as string[]) ?? [];
      return files.map((name) => ({ name, kind: "file" as const }));
    },

    write: async (path: string, contents: Uint8Array): Promise<void> => {
      await pluginCmd(pluginId, "WriteBinary", {
        path,
        data: bytesToBase64(contents),
      });
    },

    createDirAll: async (path: string): Promise<void> => {
      await pluginCmd(pluginId, "CreateDirAll", { path });
    },

    removeFile: async (path: string): Promise<void> => {
      await pluginCmd(pluginId, "DeleteFile", { path });
    },

    removeDir: async (_path: string): Promise<void> => {
      // Storage plugins (S3, Google Drive) model directories as key prefixes,
      // so removing an "empty" directory is a no-op.
    },

    removeDirAll: async (path: string): Promise<void> => {
      const result = await pluginCmd(pluginId, "ListFiles", { dir: path });
      const files = (result.files as string[]) ?? [];
      for (const name of files) {
        const childPath = path ? `${path}/${name}` : name;
        await pluginCmd(pluginId, "DeleteFile", { path: childPath });
      }
    },

    rename: async (from: string, to: string): Promise<void> => {
      await pluginCmd(pluginId, "MoveFile", { from, to });
    },

    metadata: async (
      path: string,
    ): Promise<{ kind: "file" | "dir" | "symlink"; len?: number }> => {
      const existsResult = await pluginCmd(pluginId, "Exists", { path });
      if (!(existsResult.exists as boolean)) {
        const err = new Error(`Path not found: ${path}`) as Error & {
          kind?: string;
        };
        err.kind = "NotFound";
        throw err;
      }
      const dirResult = await pluginCmd(pluginId, "IsDir", { path });
      return { kind: (dirResult.isDir as boolean) ? "dir" : "file" };
    },
  };
}
