/**
 * Persistent audience color store.
 *
 * Maps audience name strings → Tailwind background class strings
 * (e.g., "family" → "bg-indigo-500"). Persisted to localStorage.
 *
 * Follows the singleton pattern of templateContextStore.svelte.ts and
 * the localStorage persistence pattern of appearance.svelte.ts.
 *
 * IMPORTANT: Display code must always read colors through getAudienceColor()
 * from audienceDotColor.ts — never call hashAudienceColor() for display.
 */

import { hashAudienceColor } from "$lib/utils/audienceDotColor";

const STORAGE_KEY = "diaryx-audience-colors";

function load(): Record<string, string> {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) return JSON.parse(raw) as Record<string, string>;
  } catch {}
  return {};
}

function createAudienceColorStore() {
  let audienceColors = $state<Record<string, string>>(
    typeof window !== "undefined" ? load() : {},
  );

  function save() {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(audienceColors));
    } catch {}
  }

  return {
    get audienceColors(): Record<string, string> {
      return audienceColors;
    },

    /**
     * Assign a hash-based color if this audience has none yet.
     * Call at audience creation time. No-op if already assigned.
     */
    assignColor(name: string): void {
      if (name in audienceColors) return;
      audienceColors[name] = hashAudienceColor(name);
      save();
    },

    /**
     * Move the color entry when an audience is renamed.
     * Preserves the old color under the new name.
     * If oldName had no color, assigns a fresh hash color to newName.
     */
    renameColor(oldName: string, newName: string): void {
      const color = audienceColors[oldName] ?? hashAudienceColor(newName);
      delete audienceColors[oldName];
      audienceColors[newName] = color;
      save();
    },

    /** Remove the color entry when an audience is deleted. */
    deleteColor(name: string): void {
      delete audienceColors[name];
      save();
    },

    /** Called from the manual color-picker swatch UI in ManageAudiencesModal. */
    setColor(name: string, tailwindClass: string): void {
      audienceColors[name] = tailwindClass;
      save();
    },
  };
}

let sharedStore: ReturnType<typeof createAudienceColorStore> | null = null;

export function getAudienceColorStore() {
  if (typeof window === "undefined") {
    // SSR / test fallback — no-op store
    return {
      get audienceColors(): Record<string, string> {
        return {};
      },
      assignColor: (_name: string) => {},
      renameColor: (_oldName: string, _newName: string) => {},
      deleteColor: (_name: string) => {},
      setColor: (_name: string, _color: string) => {},
    };
  }
  if (!sharedStore) sharedStore = createAudienceColorStore();
  return sharedStore;
}
