/**
 * UI Store - Manages UI state
 *
 * This store holds state related to UI elements like sidebars,
 * modals, loading states, and error messages.
 */

// ============================================================================
// State
// ============================================================================

// Sidebar states - collapsed by default on mobile
let leftSidebarCollapsed = $state(true);
let rightSidebarCollapsed = $state(true);

// Sidebar widths (in pixels) - persisted to localStorage
const SIDEBAR_MIN_WIDTH = 200;
const SIDEBAR_MAX_WIDTH = 480;
const SIDEBAR_DEFAULT_WIDTH = 288; // w-72

function loadSidebarWidth(key: string): number {
  if (typeof window === 'undefined') return SIDEBAR_DEFAULT_WIDTH;
  const stored = localStorage.getItem(key);
  if (stored) {
    const n = parseInt(stored, 10);
    if (!isNaN(n) && n >= SIDEBAR_MIN_WIDTH && n <= SIDEBAR_MAX_WIDTH) return n;
  }
  return SIDEBAR_DEFAULT_WIDTH;
}

let leftSidebarWidth = $state(loadSidebarWidth('diaryx:leftSidebarWidth'));
let rightSidebarWidth = $state(loadSidebarWidth('diaryx:rightSidebarWidth'));

// Modal states
let showCommandPalette = $state(false);
let showSettingsDialog = $state(false);
let showNewEntryModal = $state(false);

// Error state
let error = $state<string | null>(null);

// Editor reference (for accessing editor methods)
let editorRef = $state<any>(null);

// ============================================================================
// Store Factory
// ============================================================================

/**
 * Get the UI store singleton.
 */
export function getUIStore() {
  return {
    // Sidebar getters
    get leftSidebarCollapsed() { return leftSidebarCollapsed; },
    get rightSidebarCollapsed() { return rightSidebarCollapsed; },
    get leftSidebarWidth() { return leftSidebarWidth; },
    get rightSidebarWidth() { return rightSidebarWidth; },
    SIDEBAR_MIN_WIDTH,
    SIDEBAR_MAX_WIDTH,

    // Modal getters/setters
    get showCommandPalette() { return showCommandPalette; },
    set showCommandPalette(value: boolean) { showCommandPalette = value; },
    get showSettingsDialog() { return showSettingsDialog; },
    get showNewEntryModal() { return showNewEntryModal; },

    // Other getters
    get error() { return error; },
    get editorRef() { return editorRef; },

    // Sidebar actions
    toggleLeftSidebar() {
      leftSidebarCollapsed = !leftSidebarCollapsed;
    },

    toggleRightSidebar() {
      rightSidebarCollapsed = !rightSidebarCollapsed;
    },

    setLeftSidebarCollapsed(collapsed: boolean) {
      leftSidebarCollapsed = collapsed;
    },

    setRightSidebarCollapsed(collapsed: boolean) {
      rightSidebarCollapsed = collapsed;
    },

    setLeftSidebarWidth(width: number) {
      leftSidebarWidth = Math.max(SIDEBAR_MIN_WIDTH, Math.min(SIDEBAR_MAX_WIDTH, width));
      if (typeof window !== 'undefined') {
        localStorage.setItem('diaryx:leftSidebarWidth', String(leftSidebarWidth));
      }
    },

    setRightSidebarWidth(width: number) {
      rightSidebarWidth = Math.max(SIDEBAR_MIN_WIDTH, Math.min(SIDEBAR_MAX_WIDTH, width));
      if (typeof window !== 'undefined') {
        localStorage.setItem('diaryx:rightSidebarWidth', String(rightSidebarWidth));
      }
    },

    // Expand sidebars (for desktop)
    expandSidebarsForDesktop() {
      if (typeof window !== 'undefined' && window.innerWidth >= 768) {
        leftSidebarCollapsed = false;
        rightSidebarCollapsed = false;
      }
    },

    // Modal actions
    openCommandPalette() { showCommandPalette = true; },
    closeCommandPalette() { showCommandPalette = false; },
    toggleCommandPalette() { showCommandPalette = !showCommandPalette; },

    openSettingsDialog() { showSettingsDialog = true; },
    closeSettingsDialog() { showSettingsDialog = false; },
    setShowSettingsDialog(show: boolean) { showSettingsDialog = show; },

    openNewEntryModal() { showNewEntryModal = true; },
    closeNewEntryModal() { showNewEntryModal = false; },
    setShowNewEntryModal(show: boolean) { showNewEntryModal = show; },

    // Error management
    setError(err: string | null) { error = err; },
    clearError() { error = null; },

    // Editor reference
    setEditorRef(ref: any) { editorRef = ref; },
  };
}

// ============================================================================
// Convenience export
// ============================================================================

export const uiStore = getUIStore();
