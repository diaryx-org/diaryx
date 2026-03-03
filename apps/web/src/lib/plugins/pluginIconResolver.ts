/**
 * Lucide Icon Resolver — resolves kebab-case icon name strings to Svelte components.
 *
 * Uses dynamic import() to load individual icon files from @lucide/svelte,
 * avoiding bundling all icons. Falls back to a Puzzle icon for
 * unrecognized names.
 */

import { Puzzle } from "@lucide/svelte";
import type { Component } from "svelte";

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type IconComponent = Component<any>;

/** Cache of resolved icon components. */
const iconCache = new Map<string, IconComponent>();

/**
 * Eagerly load a Lucide icon by kebab-case name.
 *
 * Returns the resolved Svelte component, or the Puzzle fallback
 * if the name is not found.
 */
export async function loadPluginIcon(
  name: string | null | undefined,
): Promise<IconComponent> {
  if (!name) return Puzzle;

  const cached = iconCache.get(name);
  if (cached) return cached;

  try {
    const mod = await import(
      /* @vite-ignore */ `@lucide/svelte/dist/icons/${name}.js`
    );
    const icon = (mod.default ?? Puzzle) as IconComponent;
    iconCache.set(name, icon);
    return icon;
  } catch {
    iconCache.set(name, Puzzle);
    return Puzzle;
  }
}

/**
 * Get a previously loaded icon from cache, or the Puzzle fallback.
 */
export function getCachedPluginIcon(
  name: string | null | undefined,
): IconComponent {
  if (!name) return Puzzle;
  return iconCache.get(name) ?? Puzzle;
}
