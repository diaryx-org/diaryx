/**
 * Appearance store — manages theme presets, accent hue, typography, and layout.
 *
 * Orthogonal to the theme (light/dark/system) store. A user picks a preset
 * AND a mode independently.
 *
 * Follows the singleton pattern used by formattingStore.svelte.ts.
 */

import type {
  UserAppearance,
  ThemeDefinition,
  ContentWidth,
  FontFamily,
  ThemeExport,
} from "./appearance.types";
import { PRESETS } from "./appearance.presets";
import {
  applyCssVars,
  clearCssVars,
  cacheVarsForFouc,
  clearVarsCache,
  resolveEffectivePalette,
  DEFAULT_APPEARANCE,
} from "./appearance.utils";
import { getThemeStore } from "./theme.svelte";

const STORAGE_KEY = "diaryx-appearance";

// ---------------------------------------------------------------------------
// Persistence helpers
// ---------------------------------------------------------------------------

function loadAppearance(): UserAppearance {
  if (typeof window === "undefined") return DEFAULT_APPEARANCE;
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return migrateFromLegacy();
    const parsed = JSON.parse(raw) as UserAppearance;
    // Validate preset exists
    if (!PRESETS[parsed.presetId]) parsed.presetId = "default";
    return parsed;
  } catch {
    return DEFAULT_APPEARANCE;
  }
}

function saveAppearance(a: UserAppearance): void {
  if (typeof window === "undefined") return;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(a));
  } catch {
    // Ignore quota errors
  }
}

/** Migrate from the legacy `readableLineLength` boolean. */
function migrateFromLegacy(): UserAppearance {
  const appearance = { ...DEFAULT_APPEARANCE, typography: { ...DEFAULT_APPEARANCE.typography }, layout: { ...DEFAULT_APPEARANCE.layout } };
  if (typeof window === "undefined") return appearance;

  const legacy = localStorage.getItem("diaryx-readable-line-length");
  if (legacy === "false") {
    appearance.layout.contentWidth = "full";
  }

  return appearance;
}

// ---------------------------------------------------------------------------
// Store factory
// ---------------------------------------------------------------------------

export function createAppearanceStore() {
  let appearance = $state<UserAppearance>(loadAppearance());

  /** Resolve and apply the current appearance as CSS variables. */
  function apply() {
    if (typeof document === "undefined") return;

    const preset = PRESETS[appearance.presetId];
    if (!preset) return;

    const themeStore = getThemeStore();
    const mode = themeStore.resolvedTheme;

    const isDefault =
      appearance.presetId === "default" &&
      appearance.accentHue === null &&
      appearance.typography.fontFamily === "inter" &&
      appearance.typography.baseFontSize === 16 &&
      appearance.typography.lineHeight === 1.6 &&
      appearance.layout.contentWidth === "medium";

    if (isDefault) {
      clearCssVars();
      clearVarsCache();
      return;
    }

    const palette = resolveEffectivePalette(preset, mode, appearance.accentHue);
    const vars = applyCssVars(palette, appearance.typography, appearance.layout);
    cacheVarsForFouc(vars);
  }

  // Apply on init
  apply();

  // Re-apply when light/dark mode changes
  if (typeof window !== "undefined") {
    const themeStore = getThemeStore();
    themeStore.onModeChange(() => apply());
  }

  // ---------------------------------------------------------------------------
  // Public API
  // ---------------------------------------------------------------------------

  function update(partial: Partial<UserAppearance>) {
    appearance = {
      ...appearance,
      ...partial,
      typography: { ...appearance.typography, ...(partial.typography ?? {}) },
      layout: { ...appearance.layout, ...(partial.layout ?? {}) },
    };
    saveAppearance(appearance);
    apply();
  }

  return {
    get appearance() {
      return appearance;
    },
    get presetId() {
      return appearance.presetId;
    },
    get accentHue() {
      return appearance.accentHue;
    },
    get typography() {
      return appearance.typography;
    },
    get layout() {
      return appearance.layout;
    },

    /** Get the active ThemeDefinition. */
    get activePreset(): ThemeDefinition {
      return PRESETS[appearance.presetId] ?? PRESETS.default;
    },

    setPreset(presetId: string) {
      if (!PRESETS[presetId]) return;
      update({ presetId });
    },

    setAccentHue(hue: number | null) {
      update({ accentHue: hue });
    },

    setFontFamily(fontFamily: FontFamily) {
      update({ typography: { ...appearance.typography, fontFamily } });
    },

    setBaseFontSize(baseFontSize: number) {
      update({ typography: { ...appearance.typography, baseFontSize } });
    },

    setLineHeight(lineHeight: number) {
      update({ typography: { ...appearance.typography, lineHeight } });
    },

    setContentWidth(contentWidth: ContentWidth) {
      update({ layout: { contentWidth } });
    },

    /** Re-apply CSS variables (call after light/dark mode changes). */
    reapply: apply,

    /** Reset to defaults. */
    reset() {
      appearance = { ...DEFAULT_APPEARANCE, typography: { ...DEFAULT_APPEARANCE.typography }, layout: { ...DEFAULT_APPEARANCE.layout } };
      saveAppearance(appearance);
      clearCssVars();
      clearVarsCache();
    },

    /** Export the active preset as a ThemeExport JSON object. */
    exportTheme(): ThemeExport {
      const preset = PRESETS[appearance.presetId] ?? PRESETS.default;
      return {
        $schema: "https://diaryx.com/schemas/theme/v1",
        theme: preset,
      };
    },

    /** Import a theme from a ThemeExport object. Returns true on success. */
    importTheme(data: unknown): boolean {
      try {
        const obj = data as ThemeExport;
        if (!obj?.theme?.id || !obj.theme.colors?.light || !obj.theme.colors?.dark) {
          return false;
        }
        if (obj.theme.version !== 1) return false;

        // Register as a runtime preset (not persisted as a built-in)
        const def = obj.theme as ThemeDefinition;
        PRESETS[def.id] = def;
        update({ presetId: def.id });
        return true;
      } catch {
        return false;
      }
    },
  };
}

// ---------------------------------------------------------------------------
// Singleton
// ---------------------------------------------------------------------------

let sharedStore: ReturnType<typeof createAppearanceStore> | null = null;

export function getAppearanceStore() {
  if (typeof window === "undefined") {
    // SSR fallback
    return {
      get appearance() { return DEFAULT_APPEARANCE; },
      get presetId() { return "default"; },
      get accentHue() { return null; },
      get typography() { return DEFAULT_APPEARANCE.typography; },
      get layout() { return DEFAULT_APPEARANCE.layout; },
      get activePreset() { return PRESETS.default; },
      setPreset: () => {},
      setAccentHue: () => {},
      setFontFamily: () => {},
      setBaseFontSize: () => {},
      setLineHeight: () => {},
      setContentWidth: () => {},
      reapply: () => {},
      reset: () => {},
      exportTheme: () => ({ $schema: "", theme: PRESETS.default }),
      importTheme: () => false,
    } as ReturnType<typeof createAppearanceStore>;
  }

  if (!sharedStore) {
    sharedStore = createAppearanceStore();
  }
  return sharedStore;
}
