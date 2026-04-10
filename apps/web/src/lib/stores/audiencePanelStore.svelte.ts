/**
 * Audience Panel UI state store.
 *
 * Manages the floating audience panel's open/close state, mode (view vs paint),
 * and the active paint brushes. Follows the same singleton pattern as other lib stores.
 */

export type AudiencePanelMode = "view" | "paint";

/** Special brush value that clears visibility from text/entries. Mutually
 *  exclusive with audience brushes — selecting Clear drops audiences, selecting
 *  an audience drops Clear. */
export const CLEAR_BRUSH = "__clear__";

/** Callback registered by the Editor to apply the brush to the current selection. */
export type ApplyPaintBrushFn = () => boolean;

function createAudiencePanelStore() {
  let panelOpen = $state(false);
  let mode = $state<AudiencePanelMode>("view");
  // Ordered list of picked brushes. Either a list of audience names (in the
  // order the user picked them) or exactly [CLEAR_BRUSH] — never a mix.
  let paintBrushes = $state<string[]>([]);
  let applyPaintBrushFn = $state<ApplyPaintBrushFn | null>(null);
  // True iff the editor currently has a non-empty text selection. The editor
  // pushes updates to this flag so the panel's "Apply to selection" button can
  // hide when there's nothing to apply to.
  let hasEditorSelection = $state(false);

  // A transient brush is an audience the user just typed in but hasn't yet
  // applied to any file. It lives only in panel state until either (a) a file
  // gets painted with it (then it persists in frontmatter and shows up via
  // getAvailableAudiences), or (b) the user toggles it out of the picked set
  // without painting, in which case it vanishes.
  let transientAudience = $state<string | null>(null);
  let transientPainted = $state(false);

  function dropTransientIfUnpainted() {
    if (transientAudience !== null && !transientPainted) {
      transientAudience = null;
    }
  }

  return {
    get panelOpen() {
      return panelOpen;
    },

    get mode() {
      return mode;
    },

    /** The active paint brushes, in pick order. Empty means no brush is active. */
    get paintBrushes(): readonly string[] {
      return paintBrushes;
    },

    /** Name of an audience that exists only in panel state (not yet on disk), or null. */
    get transientAudience() {
      return transientAudience;
    },

    get hasEditorSelection() {
      return hasEditorSelection;
    },

    openPanel(initialMode?: AudiencePanelMode) {
      panelOpen = true;
      if (initialMode) mode = initialMode;
    },

    closePanel() {
      panelOpen = false;
      paintBrushes = [];
      transientAudience = null;
      transientPainted = false;
    },

    setMode(newMode: AudiencePanelMode) {
      mode = newMode;
      if (newMode === "view") {
        paintBrushes = [];
        transientAudience = null;
        transientPainted = false;
      }
    },

    /** Toggle a brush in the picked set. CLEAR_BRUSH is mutually exclusive
     *  with audience brushes — picking Clear wipes audiences, picking an
     *  audience wipes Clear. */
    toggleBrush(name: string) {
      if (name === CLEAR_BRUSH) {
        if (paintBrushes.length === 1 && paintBrushes[0] === CLEAR_BRUSH) {
          paintBrushes = [];
        } else {
          // Switching from audiences → clear: drop any unpainted transient.
          dropTransientIfUnpainted();
          paintBrushes = [CLEAR_BRUSH];
        }
        return;
      }
      // Audience brush toggle
      if (paintBrushes.length === 1 && paintBrushes[0] === CLEAR_BRUSH) {
        paintBrushes = [name];
        return;
      }
      if (paintBrushes.includes(name)) {
        paintBrushes = paintBrushes.filter((b) => b !== name);
        if (name === transientAudience && !transientPainted) {
          transientAudience = null;
        }
      } else {
        paintBrushes = [...paintBrushes, name];
      }
    },

    /** Explicit set (used by the rename flow to swap an old name for a new one). */
    setBrushes(next: readonly string[]) {
      paintBrushes = [...next];
      if (
        transientAudience !== null &&
        !transientPainted &&
        !paintBrushes.includes(transientAudience)
      ) {
        transientAudience = null;
      }
    },

    /** Create a brand-new audience as a transient brush and add it to the
     *  picked set. The audience does not exist in any file's frontmatter yet —
     *  it materializes only when the user paints something with it. */
    createTransientBrush(name: string) {
      transientAudience = name;
      transientPainted = false;
      // Selecting an audience brush is mutually exclusive with CLEAR_BRUSH.
      if (paintBrushes.length === 1 && paintBrushes[0] === CLEAR_BRUSH) {
        paintBrushes = [name];
      } else if (!paintBrushes.includes(name)) {
        paintBrushes = [...paintBrushes, name];
      }
    },

    /** Called by the host after a successful paint operation. If the transient
     *  audience is among the active brushes, this marks it as "real" so
     *  toggling it off later won't drop it. */
    notePainted() {
      if (
        transientAudience !== null &&
        paintBrushes.includes(transientAudience)
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

    /** Pushed by the editor whenever its text selection changes. */
    setHasEditorSelection(value: boolean) {
      hasEditorSelection = value;
    },

    /** Register the editor's paint-apply callback. Called by Editor.svelte on mount. */
    registerApplyPaintBrush(fn: ApplyPaintBrushFn | null) {
      applyPaintBrushFn = fn;
    },

    /** Apply the active brushes to the current editor text selection. Returns true if applied. */
    applyBrushToSelection(): boolean {
      return applyPaintBrushFn?.() ?? false;
    },
  };
}

let sharedStore: ReturnType<typeof createAudiencePanelStore> | null = null;

export function getAudiencePanelStore() {
  if (typeof window === "undefined") {
    // SSR fallback
    const emptyList: readonly string[] = [];
    return {
      get panelOpen() {
        return false;
      },
      get mode(): AudiencePanelMode {
        return "view";
      },
      get paintBrushes(): readonly string[] {
        return emptyList;
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
      toggleBrush: (_name: string) => {},
      setBrushes: (_next: readonly string[]) => {},
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
