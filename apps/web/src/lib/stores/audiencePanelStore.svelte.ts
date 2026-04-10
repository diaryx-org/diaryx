/**
 * Audience Panel UI state store.
 *
 * Manages the floating audience panel's open/close state, mode (view vs paint),
 * and the active paint brush. Follows the same singleton pattern as other lib stores.
 */

export type AudiencePanelMode = "view" | "paint";

/** Special brush value that clears visibility from text/entries. */
export const CLEAR_BRUSH = "__clear__";

/** Callback registered by the Editor to apply the brush to the current selection. */
export type ApplyPaintBrushFn = () => boolean;

function createAudiencePanelStore() {
  let panelOpen = $state(false);
  let mode = $state<AudiencePanelMode>("view");
  let paintBrush = $state<string | null>(null);
  let applyPaintBrushFn = $state<ApplyPaintBrushFn | null>(null);

  return {
    get panelOpen() {
      return panelOpen;
    },

    get mode() {
      return mode;
    },

    /** The active paint brush — an audience name, CLEAR_BRUSH, or null (no brush). */
    get paintBrush() {
      return paintBrush;
    },

    openPanel(initialMode?: AudiencePanelMode) {
      panelOpen = true;
      if (initialMode) mode = initialMode;
    },

    closePanel() {
      panelOpen = false;
      paintBrush = null;
    },

    setMode(newMode: AudiencePanelMode) {
      mode = newMode;
      if (newMode === "view") {
        paintBrush = null;
      }
    },

    setBrush(name: string | null) {
      paintBrush = name;
    },

    /** Register the editor's paint-apply callback. Called by Editor.svelte on mount. */
    registerApplyPaintBrush(fn: ApplyPaintBrushFn | null) {
      applyPaintBrushFn = fn;
    },

    /** Apply the active brush to the current editor text selection. Returns true if applied. */
    applyBrushToSelection(): boolean {
      return applyPaintBrushFn?.() ?? false;
    },
  };
}

let sharedStore: ReturnType<typeof createAudiencePanelStore> | null = null;

export function getAudiencePanelStore() {
  if (typeof window === "undefined") {
    // SSR fallback
    return {
      get panelOpen() {
        return false;
      },
      get mode(): AudiencePanelMode {
        return "view";
      },
      get paintBrush() {
        return null as string | null;
      },
      openPanel: (_mode?: AudiencePanelMode) => {},
      closePanel: () => {},
      setMode: (_mode: AudiencePanelMode) => {},
      setBrush: (_name: string | null) => {},
      registerApplyPaintBrush: (_fn: ApplyPaintBrushFn | null) => {},
      applyBrushToSelection: () => false,
    };
  }

  if (!sharedStore) {
    sharedStore = createAudiencePanelStore();
  }
  return sharedStore;
}
