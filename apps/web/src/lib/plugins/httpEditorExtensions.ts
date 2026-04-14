/**
 * HTTP Editor Extensions — generates TipTap extensions from native backend
 * plugin manifests, routing render calls through the HTTP API.
 *
 * Used when the HTTP backend (`diaryx edit`) has native plugin support.
 * The CLI backend loads plugins via diaryx_extism and exposes their
 * manifests; this module creates TipTap extensions that call back into
 * the native plugin via the `/api/plugins/{id}/render` endpoint.
 *
 * Mirrors `tauriEditorExtensions.ts` but uses `fetch()` instead of
 * Tauri IPC.
 */

import { isHttpBackend, getHttpApiUrl } from "$lib/backend/interface";
import { getPluginStore } from "@/models/stores/pluginStore.svelte";
import {
  createExtensionFromManifest,
  createMarkFromManifest,
  getBuiltinExtension,
  isEditorExtension,
  type EditorExtensionManifest,
  type EditorExtensionContext,
  type RenderFn,
} from "./editorExtensionFactory";

/** Create a render function that routes through the HTTP API. */
function createHttpRenderFn(
  apiUrl: string,
  pluginId: string,
  exportName: string,
): RenderFn {
  return async (source, displayMode) => {
    const input = JSON.stringify({ source, display_mode: displayMode });
    const res = await fetch(
      `${apiUrl}/api/plugins/${encodeURIComponent(pluginId)}/render`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ export_name: exportName, input }),
      },
    );
    if (!res.ok) {
      throw new Error(`Plugin render failed: ${res.statusText}`);
    }
    const result = await res.text();
    return JSON.parse(result);
  };
}

/**
 * Get TipTap extensions from backend (native) plugin manifests.
 * Used when the HTTP backend has native plugin support. The CLI backend
 * loads plugins at startup, so extensions are available immediately via
 * the plugin store's backend manifests.
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function getHttpEditorExtensions(): any[] {
  if (!isHttpBackend()) return [];
  const apiUrl = getHttpApiUrl();
  if (!apiUrl) return [];

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const extensions: any[] = [];
  for (const manifest of getPluginStore().manifests) {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const uiEntries = (manifest.ui ?? []) as any[];
    for (const ui of uiEntries) {
      if (!isEditorExtension(ui)) continue;
      const ext = ui as EditorExtensionManifest;

      // Builtin node type — use host-registered extensions
      if (typeof ext.node_type === "object" && "Builtin" in ext.node_type) {
        const builtins = getBuiltinExtension(
          ext.node_type.Builtin.host_extension_id,
        );
        if (builtins) extensions.push(...builtins);
        continue;
      }

      if (ext.node_type === "InlineMark") {
        extensions.push(createMarkFromManifest(ext));
      } else if (ext.edit_mode === "Iframe" && ext.iframe_component_id) {
        const pluginId = String(manifest.id);
        const ctx: EditorExtensionContext = {
          pluginId,
          getComponentHtml: async (componentId) => {
            try {
              const res = await fetch(
                `${apiUrl}/api/plugins/${encodeURIComponent(pluginId)}/component/${encodeURIComponent(componentId)}`,
              );
              if (!res.ok) return null;
              return await res.text();
            } catch {
              return null;
            }
          },
        };
        extensions.push(createExtensionFromManifest(ext, null, {}, ctx));
      } else if (ext.render_export) {
        const renderFn = createHttpRenderFn(
          apiUrl,
          String(manifest.id),
          ext.render_export,
        );
        extensions.push(createExtensionFromManifest(ext, renderFn));
      }
    }
  }
  return extensions;
}
