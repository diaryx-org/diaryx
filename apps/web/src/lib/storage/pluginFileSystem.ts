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

export function createPluginFileSystemCallbacks(
  pluginId: string,
): JsFileSystemCallbacks {
  return {
    readToString: async (path: string): Promise<string> => {
      const result = await pluginCmd(pluginId, "ReadFile", { path });
      return result.content as string;
    },

    writeFile: async (path: string, content: string): Promise<void> => {
      await pluginCmd(pluginId, "WriteFile", { path, content });
    },

    deleteFile: async (path: string): Promise<void> => {
      await pluginCmd(pluginId, "DeleteFile", { path });
    },

    exists: async (path: string): Promise<boolean> => {
      const result = await pluginCmd(pluginId, "Exists", { path });
      return result.exists as boolean;
    },

    isDir: async (path: string): Promise<boolean> => {
      const result = await pluginCmd(pluginId, "IsDir", { path });
      return result.isDir as boolean;
    },

    listFiles: async (dir: string): Promise<string[]> => {
      const result = await pluginCmd(pluginId, "ListFiles", { dir });
      return result.files as string[];
    },

    listMdFiles: async (dir: string): Promise<string[]> => {
      const result = await pluginCmd(pluginId, "ListMdFiles", { dir });
      return result.files as string[];
    },

    createDirAll: async (path: string): Promise<void> => {
      await pluginCmd(pluginId, "CreateDirAll", { path });
    },

    moveFile: async (from: string, to: string): Promise<void> => {
      await pluginCmd(pluginId, "MoveFile", { from, to });
    },

    readBinary: async (path: string): Promise<Uint8Array> => {
      const result = await pluginCmd(pluginId, "ReadBinary", { path });
      const b64 = result.data as string;
      const binary = atob(b64);
      const bytes = new Uint8Array(binary.length);
      for (let i = 0; i < binary.length; i++) {
        bytes[i] = binary.charCodeAt(i);
      }
      return bytes;
    },

    writeBinary: async (
      path: string,
      data: Uint8Array,
    ): Promise<void> => {
      let binary = "";
      for (let i = 0; i < data.length; i++) {
        binary += String.fromCharCode(data[i]);
      }
      const b64 = btoa(binary);
      await pluginCmd(pluginId, "WriteBinary", { path, data: b64 });
    },
  };
}
