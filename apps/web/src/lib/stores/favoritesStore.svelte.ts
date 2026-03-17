/**
 * Favorites store for command palette favorite commands.
 * Persists favorite command IDs to localStorage with drag-and-drop reordering.
 */

const STORAGE_KEY = "diaryx:command-favorites";
const DEFAULT_FAVORITES = ["insert:add-photo"];

function load(): string[] {
  if (typeof localStorage === "undefined") return [...DEFAULT_FAVORITES];
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [...DEFAULT_FAVORITES];
    const parsed = JSON.parse(raw);
    if (Array.isArray(parsed) && parsed.every((v) => typeof v === "string")) {
      return parsed;
    }
    return [...DEFAULT_FAVORITES];
  } catch {
    return [...DEFAULT_FAVORITES];
  }
}

function persist(ids: string[]): void {
  if (typeof localStorage === "undefined") return;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(ids));
  } catch {
    // Ignore storage errors
  }
}

function createFavoritesStore() {
  let favoriteIds = $state<string[]>(load());

  return {
    get ids(): string[] {
      return favoriteIds;
    },

    isFavorite(id: string): boolean {
      return favoriteIds.includes(id);
    },

    addFavorite(id: string): void {
      if (favoriteIds.includes(id)) return;
      favoriteIds = [...favoriteIds, id];
      persist(favoriteIds);
    },

    removeFavorite(id: string): void {
      favoriteIds = favoriteIds.filter((fid) => fid !== id);
      persist(favoriteIds);
    },

    toggleFavorite(id: string): void {
      if (favoriteIds.includes(id)) {
        this.removeFavorite(id);
      } else {
        this.addFavorite(id);
      }
    },

    reorder(fromIndex: number, toIndex: number): void {
      if (
        fromIndex < 0 ||
        fromIndex >= favoriteIds.length ||
        toIndex < 0 ||
        toIndex >= favoriteIds.length ||
        fromIndex === toIndex
      ) {
        return;
      }
      const next = [...favoriteIds];
      const [item] = next.splice(fromIndex, 1);
      next.splice(toIndex, 0, item);
      favoriteIds = next;
      persist(favoriteIds);
    },
  };
}

let sharedStore: ReturnType<typeof createFavoritesStore> | null = null;

export function getFavoritesStore() {
  if (typeof window === "undefined") {
    // SSR fallback
    return {
      get ids(): string[] {
        return [];
      },
      isFavorite: () => false,
      addFavorite: () => {},
      removeFavorite: () => {},
      toggleFavorite: () => {},
      reorder: () => {},
    };
  }
  if (!sharedStore) sharedStore = createFavoritesStore();
  return sharedStore;
}
