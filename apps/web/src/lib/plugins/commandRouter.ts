/**
 * Command Router — routes typed commands to browser Extism plugins.
 *
 * When diaryx_wasm doesn't compile-link a plugin (e.g., publish, sync),
 * the frontend intercepts execute() calls and routes them to the browser
 * Extism plugin that handles the command.
 */

import {
  getPlugin,
  getBrowserManifests,
} from "./browserPluginManager.svelte";
import { getPluginStore } from "@/models/stores/pluginStore.svelte";
import type { Command, Response } from "$lib/backend/interface";

/**
 * Find a loaded browser plugin that handles the given command type.
 */
function findPluginForCommand(commandType: string) {
  const store = getPluginStore();
  const manifests = getBrowserManifests();

  for (const manifest of manifests) {
    const id = manifest.id as unknown as string;
    if (!store.isPluginEnabled(id)) continue;

    for (const cap of manifest.capabilities) {
      if (typeof cap === "object" && cap !== null && "CustomCommands" in cap) {
        const commands = (cap as { CustomCommands: { commands: string[] } })
          .CustomCommands.commands;
        if (commands.includes(commandType)) {
          return getPlugin(id);
        }
      }
    }
  }

  return undefined;
}

/**
 * Try to handle a command via a browser Extism plugin.
 *
 * Checks if any loaded browser plugin declares support for the command type,
 * then calls its `execute_typed_command` export. Returns the Response if
 * handled, null if no plugin handles it.
 */
export async function tryBrowserPluginCommand(
  command: Command,
): Promise<Response | null> {
  const commandType = (command as { type?: string }).type;
  if (!commandType) return null;

  const plugin = findPluginForCommand(commandType);
  if (!plugin) return null;

  const response = await plugin.callTypedCommand(command);
  return response as Response | null;
}
