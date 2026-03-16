/**
 * Tauri Editor Extensions — generates TipTap extensions from native backend
 * plugin manifests, routing render calls through Tauri IPC.
 *
 * Used when browser Extism plugins aren't available (iOS WKWebView lacks JSPI)
 * or haven't loaded yet. The native Tauri backend loads plugins and exposes
 * their manifests; this module creates TipTap extensions that call back into
 * the native plugin via `call_plugin_render`.
 */

import { isTauri } from "$lib/backend/interface";
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

/** Create a render function that routes through Tauri IPC. */
function createTauriRenderFn(pluginId: string, exportName: string): RenderFn {
  return async (source, displayMode) => {
    const { invoke } = await import("@tauri-apps/api/core");
    const input = JSON.stringify({ source, display_mode: displayMode });
    const result = await invoke<string>("call_plugin_render", {
      pluginId,
      exportName,
      input,
    });
    return JSON.parse(result);
  };
}

/**
 * Get TipTap extensions from backend (native) plugin manifests.
 * Used on all Tauri platforms (desktop + iOS). The native backend loads
 * plugins synchronously at startup, so extensions are available immediately
 * — unlike browser Extism plugins which load asynchronously.
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function getTauriEditorExtensions(): any[] {
  if (!isTauri()) return [];

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
            const { invoke } = await import("@tauri-apps/api/core");
            try {
              return await invoke<string>("get_plugin_component_html", {
                pluginId,
                componentId,
              });
            } catch {
              return null;
            }
          },
        };
        extensions.push(createExtensionFromManifest(ext, null, {}, ctx));
      } else if (ext.render_export) {
        const renderFn = createTauriRenderFn(
          String(manifest.id),
          ext.render_export,
        );
        extensions.push(createExtensionFromManifest(ext, renderFn));
      }
    }
  }
  return extensions;
}
