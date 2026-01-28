/**
 * Formatting store for managing editor formatting preferences.
 * Uses Svelte 5 runes for reactive state management.
 * Persists preferences to localStorage.
 */

const SPOILERS_STORAGE_KEY = "diaryx-enable-spoilers";

/**
 * Creates reactive formatting state with persistence.
 */
export function createFormattingStore() {
  let enableSpoilers = $state(true);

  // Initialize from localStorage
  if (typeof window !== "undefined") {
    const stored = localStorage.getItem(SPOILERS_STORAGE_KEY);
    if (stored !== null) {
      enableSpoilers = stored === "true";
    }
  }

  function setEnableSpoilers(value: boolean) {
    enableSpoilers = value;

    // Persist to localStorage
    if (typeof localStorage !== "undefined") {
      localStorage.setItem(SPOILERS_STORAGE_KEY, String(value));
    }
  }

  return {
    get enableSpoilers() {
      return enableSpoilers;
    },
    setEnableSpoilers,
  };
}

/**
 * Singleton instance for shared formatting state across components.
 */
let sharedFormattingStore: ReturnType<typeof createFormattingStore> | null = null;

export function getFormattingStore() {
  if (typeof window === "undefined") {
    // SSR fallback
    return {
      get enableSpoilers() {
        return true;
      },
      setEnableSpoilers: () => {},
    };
  }

  if (!sharedFormattingStore) {
    sharedFormattingStore = createFormattingStore();
  }
  return sharedFormattingStore;
}
