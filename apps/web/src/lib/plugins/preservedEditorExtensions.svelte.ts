import type { PluginManifest } from "$lib/backend/generated";
import {
  createExtensionFromManifest,
  createMarkFromManifest,
  getBuiltinExtension,
  isEditorExtension,
  type EditorExtensionManifest,
} from "./editorExtensionFactory";

interface PreservedPluginExtensionsEntry {
  pluginId: string;
  pluginName: string;
  extensions: EditorExtensionManifest[];
}

let preservedByPluginId = $state<Record<string, PreservedPluginExtensionsEntry>>(
  {},
);
let preservedVersion = $state(0);

// eslint-disable-next-line @typescript-eslint/no-explicit-any
let cachedPreservedExtensions: any[] | null = null;
let cachedPreservedVersion = -1;

function invalidatePreservedExtensions(): void {
  cachedPreservedExtensions = null;
  preservedVersion++;
}

export function preservePluginEditorExtensions(
  manifest: PluginManifest | null | undefined,
): void {
  if (!manifest) return;

  const pluginId = String(manifest.id);
  const extensions = ((manifest.ui ?? []) as unknown[])
    .filter(isEditorExtension)
    .map((ui) => ui as EditorExtensionManifest);

  if (extensions.length === 0) return;

  preservedByPluginId = {
    ...preservedByPluginId,
    [pluginId]: {
      pluginId,
      pluginName: String(manifest.name ?? pluginId),
      extensions,
    },
  };
  invalidatePreservedExtensions();
}

export function clearPreservedPluginEditorExtensions(pluginId: string): void {
  if (!(pluginId in preservedByPluginId)) return;
  const next = { ...preservedByPluginId };
  delete next[pluginId];
  preservedByPluginId = next;
  invalidatePreservedExtensions();
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function getPreservedEditorExtensions(): any[] {
  if (
    cachedPreservedExtensions &&
    cachedPreservedVersion === preservedVersion
  ) {
    return cachedPreservedExtensions;
  }

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const extensions: any[] = [];

  for (const entry of Object.values(preservedByPluginId)) {
    for (const manifest of entry.extensions) {
      if (
        typeof manifest.node_type === "object" &&
        "Builtin" in manifest.node_type
      ) {
        const builtinExts = getBuiltinExtension(
          manifest.node_type.Builtin.host_extension_id,
        );
        if (builtinExts) {
          extensions.push(...builtinExts);
        }
        continue;
      }

      if (manifest.node_type === "InlineMark") {
        extensions.push(
          createMarkFromManifest(manifest, {
            pluginName: entry.pluginName,
            preserveOnly: true,
          }),
        );
        continue;
      }

      extensions.push(
        createExtensionFromManifest(
          manifest,
          async () => ({
            error: `${entry.pluginName} plugin removed`,
          }),
          {
            pluginName: entry.pluginName,
            preserveOnly: true,
          },
        ),
      );
    }
  }

  cachedPreservedExtensions = extensions;
  cachedPreservedVersion = preservedVersion;
  return extensions;
}
