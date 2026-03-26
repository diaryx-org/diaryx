/**
 * Shared composable for marketplace panel state, filtering, sorting, and local import.
 *
 * Each panel (Starters, Templates, Bundles, Themes, Plugins) shares the same
 * search/filter/sort pipeline and local-file-import flow.  This composable
 * captures that logic so individual panels only keep domain-specific actions
 * and detail-view templates.
 */

/* ------------------------------------------------------------------ */
/*  Public types                                                       */
/* ------------------------------------------------------------------ */

/** Minimum shape every marketplace item must satisfy. */
export interface MarketplaceItem {
  id: string;
  name: string;
  summary: string;
  description: string;
  author: string;
  categories: string[];
  tags: string[];
}

export type SortOption = "name" | "recent" | "version";

export interface ExtraFilterDimension<T> {
  /** Reactive list of options (first element should be "all"). */
  getOptions: (items: T[]) => string[];
  /** Current filter value – the composable stores it. */
  initial: string;
  /** Return false to exclude an item when the filter is not "all". */
  matches: (item: T, filterValue: string) => boolean;
}

export interface MarketplacePanelConfig<T extends MarketplaceItem> {
  /** Fetch the full item list.  Called on mount and on `reload()`. */
  fetchItems: () => Promise<T[]>;

  /**
   * Build a search haystack from an item.
   * Default: [id, name, summary, description, author, ...tags, ...categories]
   */
  buildHaystack?: (item: T) => string[];

  /** Extract a numeric timestamp for the "recent" sort.  Default: 0. */
  getTimestamp?: (item: T) => number;

  /** Available sort options.  Default: ["name", "recent"]. */
  sortOptions?: SortOption[];

  /** Source filter `<select>` options as `[value, label]` pairs. */
  sourceFilterOptions?: [string, string][];

  /**
   * Return false to exclude an item for the current source filter value.
   * Only called when sourceFilter !== "all".
   */
  matchesSourceFilter?: (item: T, filter: string) => boolean;

  /** Extra filter dimensions (e.g. capability for plugins, style for themes). */
  extraFilters?: ExtraFilterDimension<T>[];

  /**
   * Parse a local JSON payload into item(s).
   * If provided, local-import UI is enabled.
   */
  parseLocalPayload?: (payload: unknown) => T[];
}

/* ------------------------------------------------------------------ */
/*  Utility                                                            */
/* ------------------------------------------------------------------ */

/** Type guard duplicated across Themes / Bundles – exported for reuse. */
export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

/* ------------------------------------------------------------------ */
/*  Composable                                                         */
/* ------------------------------------------------------------------ */

export function useMarketplacePanel<T extends MarketplaceItem>(
  config: MarketplacePanelConfig<T>,
) {
  /* ---------- internal $state ---------- */
  let _items = $state<T[]>([]);
  let _loading = $state(true);
  let _loadError = $state<string | null>(null);

  let _search = $state("");
  let _filtersOpen = $state(false);
  let _categoryFilter = $state("all");
  let _sourceFilter = $state("all");
  let _sortBy = $state<SortOption>(config.sortOptions?.[0] ?? "name");

  let _detailId = $state<string | null>(null);
  let _importingLocal = $state(false);
  let _localFileInputRef = $state<HTMLInputElement | null>(null);

  let _activeIds = $state<Set<string>>(new Set());

  /* Extra filter state – one per dimension */
  const extraFilterValues: string[] = $state(
    (config.extraFilters ?? []).map((f) => f.initial),
  );

  /* ---------- derived ---------- */

  const categories = $derived.by(() => {
    const all = new Set<string>();
    for (const item of _items) {
      for (const cat of item.categories) all.add(cat);
    }
    return ["all", ...Array.from(all).sort()];
  });

  const extraFilterOptions = $derived.by(() =>
    (config.extraFilters ?? []).map((dim) => dim.getOptions(_items)),
  );

  const filteredItems = $derived.by(() => {
    const query = _search.trim().toLowerCase();
    const buildHaystack =
      config.buildHaystack ??
      ((item: T) => [
        item.id,
        item.name,
        item.summary,
        item.description,
        item.author,
        ...item.tags,
        ...item.categories,
      ]);

    const filtered = _items.filter((item) => {
      // Source filter
      if (_sourceFilter !== "all") {
        if (config.matchesSourceFilter) {
          if (!config.matchesSourceFilter(item, _sourceFilter)) return false;
        }
      }

      // Category filter
      if (_categoryFilter !== "all" && !item.categories.includes(_categoryFilter)) {
        return false;
      }

      // Extra filters
      const extras = config.extraFilters ?? [];
      for (let i = 0; i < extras.length; i++) {
        const val = extraFilterValues[i];
        if (val !== "all" && !extras[i].matches(item, val)) return false;
      }

      // Search
      if (!query) return true;
      const haystack = buildHaystack(item).join(" ").toLowerCase();
      return haystack.includes(query);
    });

    // Sort
    const getTimestamp = config.getTimestamp ?? (() => 0);
    filtered.sort((a, b) => {
      if (_sortBy === "name") return a.name.localeCompare(b.name);
      if (_sortBy === "version") {
        // Only meaningful for plugins; fall back to name
        return (b as any).version?.localeCompare?.((a as any).version) ?? 0;
      }
      // "recent"
      return getTimestamp(b) - getTimestamp(a);
    });

    return filtered;
  });

  const detailItem = $derived.by(() => {
    if (!_detailId) return null;
    return _items.find((item) => item.id === _detailId) ?? null;
  });

  /* ---------- actions ---------- */

  async function reload(): Promise<void> {
    _loading = true;
    _loadError = null;
    try {
      _items = await config.fetchItems();
    } catch (error) {
      _loadError =
        error instanceof Error ? error.message : "Failed to load catalog";
      _items = [];
    } finally {
      _loading = false;
    }
  }

  function triggerLocalImport(): void {
    _localFileInputRef?.click();
  }

  /**
   * Read a local JSON file, parse it via `parseLocalPayload`, and merge into
   * the item list.  Returns the parsed items so the caller can show a toast.
   */
  async function onLocalFileSelected(event: Event): Promise<T[]> {
    const input = event.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return [];
    input.value = "";

    if (!config.parseLocalPayload) return [];

    _importingLocal = true;
    try {
      const text = await file.text();
      const parsed = JSON.parse(text);
      const newItems = config.parseLocalPayload(parsed);

      // Merge into _items (local overrides by id)
      const map = new Map(_items.map((item) => [item.id, item]));
      for (const item of newItems) {
        map.set(item.id, item);
      }
      _items = Array.from(map.values());

      return newItems;
    } catch (error) {
      throw error;
    } finally {
      _importingLocal = false;
    }
  }

  function addActiveId(id: string): void {
    _activeIds = new Set([..._activeIds, id]);
  }

  function removeActiveId(id: string): void {
    _activeIds = new Set(Array.from(_activeIds).filter((x) => x !== id));
  }

  function isActive(id: string): boolean {
    return _activeIds.has(id);
  }

  /** Replace the items array directly (useful when fetchItems does complex merging). */
  function setItems(items: T[]): void {
    _items = items;
  }

  /* ---------- kick off initial load ---------- */
  $effect(() => {
    reload();
  });

  /* ---------- return reactive surface ---------- */
  return {
    // Reactive state via getters / setters
    get items() { return _items; },
    get loading() { return _loading; },
    get loadError() { return _loadError; },

    get search() { return _search; },
    set search(v: string) { _search = v; },

    get filtersOpen() { return _filtersOpen; },
    set filtersOpen(v: boolean) { _filtersOpen = v; },

    get categoryFilter() { return _categoryFilter; },
    set categoryFilter(v: string) { _categoryFilter = v; },

    get sourceFilter() { return _sourceFilter; },
    set sourceFilter(v: string) { _sourceFilter = v; },

    get sortBy() { return _sortBy; },
    set sortBy(v: SortOption) { _sortBy = v; },

    get detailId() { return _detailId; },
    set detailId(v: string | null) { _detailId = v; },

    get importingLocal() { return _importingLocal; },

    get localFileInputRef() { return _localFileInputRef; },
    set localFileInputRef(v: HTMLInputElement | null) { _localFileInputRef = v; },

    get activeIds() { return _activeIds; },

    // Extra filter accessors (indexed)
    getExtraFilter(index: number): string { return extraFilterValues[index]; },
    setExtraFilter(index: number, value: string): void { extraFilterValues[index] = value; },
    get extraFilterOptions() { return extraFilterOptions; },

    // Derived
    get categories() { return categories; },
    get filteredItems() { return filteredItems; },
    get detailItem() { return detailItem; },

    // Actions
    reload,
    triggerLocalImport,
    onLocalFileSelected,
    addActiveId,
    removeActiveId,
    isActive,
    setItems,
  };
}
