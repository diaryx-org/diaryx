/**
 * Export Service
 *
 * Generic export orchestration that works with any export-format-contributing
 * plugin. The frontend discovers export formats from plugin manifests, collects
 * files (respecting the current audience view), delegates conversion to the
 * contributing plugin, and downloads the result as a ZIP.
 */

import type { Api, ExportedFile } from "../lib/backend";
import { toast } from "svelte-sonner";
import * as browserPlugins from "../lib/plugins/browserPluginManager.svelte";
import { getTemplateContextStore } from "../lib/stores/templateContextStore.svelte";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/** Describes an export format contributed by a plugin. */
export interface ExportFormatInfo {
  /** Format id, e.g. "pdf" */
  id: string;
  /** Human label, e.g. "PDF" */
  label: string;
  /** File extension including dot, e.g. ".pdf" */
  extension: string;
  /** Whether output is binary (true) or text (false). */
  binary: boolean;
  /** Plugin command to call for conversion, or null for raw export. */
  convertCommand: string | null;
  /** The plugin that contributes this format. */
  pluginId: string;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function uint8ArrayToBase64(data: Uint8Array): string {
  let binary = "";
  for (let i = 0; i < data.length; i++) {
    binary += String.fromCharCode(data[i]);
  }
  return btoa(binary);
}

function base64ToUint8Array(b64: string): Uint8Array {
  const binary = atob(b64);
  const data = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    data[i] = binary.charCodeAt(i);
  }
  return data;
}

/** Normalize WASM Map instances to plain objects. */
function normalizeToObject(value: unknown): unknown {
  if (value instanceof Map) {
    const obj: Record<string, unknown> = {};
    for (const [k, v] of value.entries()) {
      obj[k] = normalizeToObject(v);
    }
    return obj;
  }
  if (Array.isArray(value)) {
    return value.map(normalizeToObject);
  }
  return value;
}

async function executePluginCommand<T = unknown>(
  api: Api,
  pluginId: string,
  command: string,
  params: Record<string, unknown> = {},
): Promise<T> {
  const browserPlugin = browserPlugins.getPlugin(pluginId);
  if (browserPlugin) {
    const result = await browserPlugins.dispatchCommand(pluginId, command, params);
    if (!result.success) {
      throw new Error(result.error ?? `Plugin command failed: ${pluginId}/${command}`);
    }
    return normalizeToObject(result.data) as T;
  }
  const data = await api.executePluginCommand(pluginId, command, params as Record<string, unknown>);
  return normalizeToObject(data) as T;
}

// ---------------------------------------------------------------------------
// Built-in formats (no plugin required)
// ---------------------------------------------------------------------------

const BUILTIN_MARKDOWN: ExportFormatInfo = {
  id: "markdown",
  label: "Markdown",
  extension: ".md",
  binary: false,
  convertCommand: null,
  pluginId: "",
};

const BUILTIN_HTML: ExportFormatInfo = {
  id: "html",
  label: "HTML",
  extension: ".html",
  binary: false,
  convertCommand: null,
  pluginId: "",
};

/** The built-in export formats that are always available. */
export const BUILTIN_EXPORT_FORMATS: ExportFormatInfo[] = [
  BUILTIN_MARKDOWN,
  BUILTIN_HTML,
];

// ---------------------------------------------------------------------------
// Core export orchestration
// ---------------------------------------------------------------------------

/**
 * Run a full export for the given format.
 *
 * Audience filtering is inherited from `templateContextStore.previewAudience`.
 * If no audience is selected, all files are exported.
 */
export async function runExport(
  api: Api,
  rootPath: string,
  format: ExportFormatInfo,
): Promise<void> {
  const templateCtx = getTemplateContextStore();
  const previewAudience = templateCtx.previewAudience;

  // Convert previewAudience (string[] | null) → single audience string for the API.
  // If multiple audiences are selected, use the first one. If none, export all.
  const audience = previewAudience && previewAudience.length > 0
    ? previewAudience[0]
    : "*";

  const toastId = toast.loading(`Exporting as ${format.label}…`);

  try {
    if (format.convertCommand && format.pluginId) {
      await exportWithConversion(api, rootPath, audience, format, toastId);
    } else if (format.id === "html") {
      await exportHtml(api, rootPath, audience, toastId);
    } else {
      await exportMarkdown(api, rootPath, audience, toastId);
    }
  } catch (e) {
    toast.error(`Export failed`, {
      id: toastId,
      description: e instanceof Error ? e.message : String(e),
    });
  }
}

// ---------------------------------------------------------------------------
// Export pipelines
// ---------------------------------------------------------------------------

async function exportMarkdown(
  api: Api,
  rootPath: string,
  audience: string,
  toastId: string | number,
): Promise<void> {
  const rawFiles = await api.exportToMemory(rootPath, audience);
  const files = (normalizeToObject(rawFiles) ?? []) as ExportedFile[];
  const rawBinaries = await api.exportBinaryAttachments(rootPath, audience);
  const binaries = (normalizeToObject(rawBinaries) ?? []) as { source_path: string; relative_path: string }[];
  await downloadAsZip(api, files, binaries, rootPath, toastId);
}

async function exportHtml(
  api: Api,
  rootPath: string,
  audience: string,
  toastId: string | number,
): Promise<void> {
  const rawFiles = await api.exportToHtml(rootPath, audience);
  const files = (normalizeToObject(rawFiles) ?? []) as ExportedFile[];
  const rawBinaries = await api.exportBinaryAttachments(rootPath, audience);
  const binaries = (normalizeToObject(rawBinaries) ?? []) as { source_path: string; relative_path: string }[];
  await downloadAsZip(api, files, binaries, rootPath, toastId);
}

async function exportWithConversion(
  api: Api,
  rootPath: string,
  audience: string,
  format: ExportFormatInfo,
  toastId: string | number,
): Promise<void> {
  const rawFiles = await api.exportToMemory(rootPath, audience);
  const files = (normalizeToObject(rawFiles) ?? []) as ExportedFile[];
  const rawBinaries = await api.exportBinaryAttachments(rootPath, audience);
  const binaries = (normalizeToObject(rawBinaries) ?? []) as { source_path: string; relative_path: string }[];

  // Build base64 resource map for binary attachments
  const resources: Record<string, string> = {};
  for (const info of binaries) {
    try {
      const data = await api.readBinary(info.source_path);
      resources[info.relative_path] = uint8ArrayToBase64(data);
    } catch (e) {
      console.warn(`[Export] Failed to read binary ${info.source_path}:`, e);
    }
  }

  // Convert each file via the plugin
  const convertedFiles: { path: string; data: Uint8Array | string }[] = [];
  for (let i = 0; i < files.length; i++) {
    toast.loading(`Converting ${i + 1}/${files.length}: ${files[i].path}`, { id: toastId });
    const result = await executePluginCommand<Record<string, unknown>>(
      api,
      format.pluginId,
      format.convertCommand!,
      {
        content: files[i].content,
        from: "markdown",
        to: format.id,
        resources,
      },
    );
    const newPath = files[i].path.replace(/\.md$/, format.extension);
    if (format.binary) {
      if (!result?.binary) {
        throw new Error(`No binary output returned for ${files[i].path}`);
      }
      convertedFiles.push({ path: newPath, data: base64ToUint8Array(result.binary as string) });
    } else {
      convertedFiles.push({ path: newPath, data: String(result?.content ?? "") });
    }
  }

  await downloadConvertedAsZip(api, convertedFiles, format.binary ? [] : binaries, rootPath, toastId);
}

// ---------------------------------------------------------------------------
// ZIP download helpers
// ---------------------------------------------------------------------------

async function downloadAsZip(
  api: Api,
  files: ExportedFile[],
  binaryFileInfos: { source_path: string; relative_path: string }[],
  rootPath: string,
  toastId: string | number,
): Promise<void> {
  const JSZip = (await import("jszip")).default;
  const zip = new JSZip();

  for (const file of files) {
    zip.file(file.path, file.content);
  }

  for (const info of binaryFileInfos) {
    try {
      const data = await api.readBinary(info.source_path);
      zip.file(info.relative_path, data, { binary: true });
    } catch (e) {
      console.warn(`[Export] Failed to read binary file ${info.source_path}:`, e);
    }
  }

  triggerDownload(await zip.generateAsync({ type: "blob" }), rootPath, toastId);
}

async function downloadConvertedAsZip(
  api: Api,
  convertedFiles: { path: string; data: Uint8Array | string }[],
  binaryFileInfos: { source_path: string; relative_path: string }[],
  rootPath: string,
  toastId: string | number,
): Promise<void> {
  const JSZip = (await import("jszip")).default;
  const zip = new JSZip();

  for (const file of convertedFiles) {
    if (file.data instanceof Uint8Array) {
      zip.file(file.path, file.data, { binary: true });
    } else {
      zip.file(file.path, file.data);
    }
  }

  for (const info of binaryFileInfos) {
    try {
      const data = await api.readBinary(info.source_path);
      zip.file(info.relative_path, data, { binary: true });
    } catch (e) {
      console.warn(`[Export] Failed to read binary file ${info.source_path}:`, e);
    }
  }

  triggerDownload(await zip.generateAsync({ type: "blob" }), rootPath, toastId);
}

function triggerDownload(blob: Blob, rootPath: string, toastId: string | number): void {
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  const baseName = rootPath.split("/").pop()?.replace(".md", "") || "export";
  const filename = `${baseName}-export.zip`;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  URL.revokeObjectURL(url);

  toast.success(`Saved to ${filename}`, { id: toastId });
}
