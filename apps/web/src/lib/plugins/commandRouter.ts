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
 * Find a loaded browser plugin by its plugin ID.
 */
function findPluginById(pluginId: string) {
  const store = getPluginStore();
  if (!store.isPluginEnabled(pluginId)) return undefined;
  return getPlugin(pluginId);
}

/**
 * Try to handle a command via a browser Extism plugin.
 *
 * Handles two routing patterns:
 * 1. PluginCommand { plugin, command, params } — route by plugin ID
 * 2. Direct command type — search plugin manifests for capability match
 *
 * Returns the Response if handled, null if no plugin handles it.
 */
export async function tryBrowserPluginCommand(
  command: Command,
): Promise<Response | null> {
  const commandObj = command as Record<string, unknown>;
  const commandType = commandObj.type as string | undefined;
  if (!commandType) return null;

  // Handle PluginCommand wrapper: route by plugin ID
  if (commandType === "PluginCommand") {
    const params = commandObj.params as Record<string, unknown> | undefined;
    if (!params) return null;

    const pluginId = params.plugin as string;
    const innerCommand = params.command as string;
    const innerParams = params.params;

    const plugin = findPluginById(pluginId);
    if (!plugin) return null;

    // Call the plugin's typed command handler with the inner command
    const innerCommandObj = { type: innerCommand, params: innerParams };
    const response = await plugin.callTypedCommand(innerCommandObj);
    if (response == null) return null;

    // Wrap the plugin response as PluginResult
    return { type: "PluginResult", data: response } as Response;
  }

  // Direct command type — search by capability
  const plugin = findPluginForCommand(commandType);
  if (!plugin) return null;

  const response = await plugin.callTypedCommand(command);
  return response as Response | null;
}
