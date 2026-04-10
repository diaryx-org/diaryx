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
  // True iff the editor currently has a non-empty text selection. The editor
  // pushes updates to this flag so the panel's "Apply to selection" button can
  // hide when there's nothing to apply to.
  let hasEditorSelection = $state(false);

  // A transient brush is an audience the user just typed in but hasn't yet
  // applied to any file. It lives only in panel state until either (a) a file
  // gets painted with it (then it persists in frontmatter and shows up via
  // getAvailableAudiences), or (b) the user moves to a different brush, in
  // which case it vanishes.
  let transientAudience = $state<string | null>(null);
  let transientPainted = $state(false);

  function clearTransientIfUnpainted() {
    if (transientAudience !== null && !transientPainted) {
      transientAudience = null;
    }
    transientPainted = false;
  }

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

    /** Name of an audience that exists only in panel state (not yet on disk), or null. */
    get transientAudience() {
      return transientAudience;
    },

    openPanel(initialMode?: AudiencePanelMode) {
      panelOpen = true;
      if (initialMode) mode = initialMode;
    },

    closePanel() {
      panelOpen = false;
      paintBrush = null;
      transientAudience = null;
      transientPainted = false;
    },

    setMode(newMode: AudiencePanelMode) {
      mode = newMode;
      if (newMode === "view") {
        paintBrush = null;
        transientAudience = null;
        transientPainted = false;
      }
    },

    setBrush(name: string | null) {
      if (transientAudience !== null && name !== transientAudience) {
        clearTransientIfUnpainted();
      }
      paintBrush = name;
    },

    /** Create a brand-new audience as a transient brush. The audience does not
     *  exist in any file's frontmatter yet — it materializes only when the user
     *  paints something with it. */
    createTransientBrush(name: string) {
      transientAudience = name;
      transientPainted = false;
      paintBrush = name;
    },

    /** Called by the host after a successful paint operation. If the active
     *  brush is a transient one, this marks it as "real" so switching brushes
     *  later won't drop it. */
    notePainted() {
      if (
        transientAudience !== null &&
        paintBrush === transientAudience
      ) {
        transientPainted = true;
      }
    },

    /** Drop the transient when it has been confirmed by a fresh load of the
     *  on-disk audience list. */
    confirmTransientPersisted() {
      transientAudience = null;
      transientPainted = false;
    },

    get hasEditorSelection() {
      return hasEditorSelection;
    },

    /** Pushed by the editor whenever its text selection changes. */
    setHasEditorSelection(value: boolean) {
      hasEditorSelection = value;
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
      get transientAudience() {
        return null as string | null;
      },
      get hasEditorSelection() {
        return false;
      },
      openPanel: (_mode?: AudiencePanelMode) => {},
      closePanel: () => {},
      setMode: (_mode: AudiencePanelMode) => {},
      setBrush: (_name: string | null) => {},
      createTransientBrush: (_name: string) => {},
      notePainted: () => {},
      confirmTransientPersisted: () => {},
      setHasEditorSelection: (_value: boolean) => {},
      registerApplyPaintBrush: (_fn: ApplyPaintBrushFn | null) => {},
      applyBrushToSelection: () => false,
    };
  }

  if (!sharedStore) {
    sharedStore = createAudiencePanelStore();
  }
  return sharedStore;
}
