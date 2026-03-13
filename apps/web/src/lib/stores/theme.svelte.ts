/**
 * Theme store for managing dark/light mode.
 * Uses Svelte 5 runes for reactive state management.
 * Persists preference to localStorage and respects system preference.
 */

import {
  getThemeModePath,
  readWorkspaceText,
  writeWorkspaceText,
} from "$lib/workspace/workspaceAssetStorage";

export type ThemeMode = "light" | "dark" | "system";

const STORAGE_KEY = "diaryx-theme";

function isThemeMode(value: unknown): value is ThemeMode {
  return value === "light" || value === "dark" || value === "system";
}

/**
 * Creates reactive theme state with persistence.
 */
export function createThemeStore() {
  let mode = $state<ThemeMode>("system");
  let resolvedTheme = $state<"light" | "dark">("light");

  // Listeners notified after the resolved mode changes (used by appearance store)
  const modeChangeListeners: Array<() => void> = [];

  function applyResolvedTheme(nextMode: ThemeMode) {
    if (typeof window === "undefined") return;

    if (nextMode === "system") {
      const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
      resolvedTheme = mediaQuery.matches ? "dark" : "light";
    } else {
      resolvedTheme = nextMode;
    }

    applyTheme(resolvedTheme);
    for (const fn of modeChangeListeners) fn();
  }

  function persistLegacyMode(nextMode: ThemeMode): void {
    if (typeof localStorage === "undefined") return;
    localStorage.setItem(STORAGE_KEY, nextMode);
  }

  async function persistWorkspaceMode(nextMode: ThemeMode): Promise<void> {
    try {
      await writeWorkspaceText(
        getThemeModePath(),
        JSON.stringify({ mode: nextMode }, null, 2),
      );
    } catch (error) {
      console.warn("[themeStore] Failed to persist workspace theme mode:", error);
    }
  }

  function setModeInternal(
    newMode: ThemeMode,
    options: { persistLegacy?: boolean; persistWorkspace?: boolean } = {},
  ) {
    const { persistLegacy = true, persistWorkspace = true } = options;
    mode = newMode;

    if (persistLegacy) {
      persistLegacyMode(newMode);
    }
    if (persistWorkspace) {
      void persistWorkspaceMode(newMode);
      persistThemeMode?.(newMode);
    }

    applyResolvedTheme(newMode);
  }

  // Initialize from localStorage or default to system
  if (typeof window !== "undefined") {
    const stored = localStorage.getItem(STORAGE_KEY) as ThemeMode | null;
    if (isThemeMode(stored)) {
      mode = stored;
    }

    // Listen for system preference changes
    const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");

    function updateResolvedTheme() {
      applyResolvedTheme(mode);
    }

    mediaQuery.addEventListener("change", updateResolvedTheme);

    // Initial resolution
    updateResolvedTheme();
  }

  function applyTheme(theme: "light" | "dark") {
    if (typeof document === "undefined") return;

    const root = document.documentElement;
    if (theme === "dark") {
      root.classList.add("dark");
    } else {
      root.classList.remove("dark");
    }
  }

  function setMode(newMode: ThemeMode) {
    setModeInternal(newMode);
  }

  function toggle() {
    // Toggle between light and dark (skip system in toggle)
    setMode(resolvedTheme === "dark" ? "light" : "dark");
  }

  async function reloadFromWorkspace(): Promise<void> {
    try {
      const raw = await readWorkspaceText(getThemeModePath());
      if (!raw) {
        persistLegacyMode(mode);
        void persistWorkspaceMode(mode);
        return;
      }

      const parsed = JSON.parse(raw) as { mode?: unknown };
      if (isThemeMode(parsed.mode)) {
        setModeInternal(parsed.mode, {
          persistLegacy: true,
          persistWorkspace: false,
        });
      }
    } catch (error) {
      console.warn("[themeStore] Failed to reload workspace theme mode:", error);
    }
  }

  // Callback to persist theme_mode to workspace config; wired up by hydrateThemeMode().
  let persistThemeMode: ((mode: ThemeMode) => Promise<void>) | null = null;

  return {
    get mode() {
      return mode;
    },
    get resolvedTheme() {
      return resolvedTheme;
    },
    get isDark() {
      return resolvedTheme === "dark";
    },
    setMode,
    toggle,
    reloadFromWorkspace,
    /**
     * Hydrate theme mode from workspace config after backend init.
     * Replaces localStorage as the source of truth for theme_mode.
     */
    hydrateThemeMode(
      themeMode: string | undefined,
      persistFn: (mode: ThemeMode) => Promise<void>,
    ): void {
      persistThemeMode = persistFn;
      if (isThemeMode(themeMode)) {
        setModeInternal(themeMode, { persistLegacy: true, persistWorkspace: false });
      }
    },
    /** Register a callback invoked after light/dark mode changes. */
    onModeChange(fn: () => void) {
      modeChangeListeners.push(fn);
      return () => {
        const idx = modeChangeListeners.indexOf(fn);
        if (idx >= 0) modeChangeListeners.splice(idx, 1);
      };
    },
  };
}

/**
 * Singleton instance for shared theme state across components.
 */
let sharedThemeStore: ReturnType<typeof createThemeStore> | null = null;

export function getThemeStore() {
  if (typeof window === "undefined") {
    // SSR fallback
    return {
      get mode() {
        return "system" as ThemeMode;
      },
      get resolvedTheme() {
        return "light" as const;
      },
      get isDark() {
        return false;
      },
      setMode: () => {},
      toggle: () => {},
      reloadFromWorkspace: async () => {},
      hydrateThemeMode: () => {},
      onModeChange: () => () => {},
    };
  }

  if (!sharedThemeStore) {
    sharedThemeStore = createThemeStore();
  }
  return sharedThemeStore;
}
