/**
 * EditorGutter — Generic gutter infrastructure for the TipTap editor.
 *
 * Provides a left-side gutter column that other extensions can populate with
 * indicators (colored dots, vertical bars, collapse markers). The gutter
 * only appears when at least one indicator is registered, so there is no
 * layout cost when no directives are present.
 *
 * ## Design
 *
 * The gutter is implemented as:
 * 1. A CSS class on the editor element that reserves left padding.
 * 2. ProseMirror `Decoration.widget()` decorations positioned in the gutter.
 * 3. A registration system where extensions push indicator descriptors into
 *    shared plugin state, and the gutter plugin renders them.
 *
 * ## Usage by other extensions
 *
 * Extensions interact with the gutter through `EditorGutterState`:
 *
 * ```ts
 * // In your ProseMirror plugin's state/view:
 * const gutterState = editorGutterKey.getState(state);
 * gutterState?.registerIndicators("myExtension", indicators);
 * ```
 *
 * Or more commonly, extensions produce their own `Decoration.widget()` calls
 * using the gutter CSS utilities, and this plugin just manages the gutter
 * space (padding) based on whether any gutter-using extensions are active.
 */

import { Extension } from "@tiptap/core";
import {
  Plugin as ProseMirrorPlugin,
  PluginKey,
} from "@tiptap/pm/state";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface GutterIndicator {
  /** Document position where the indicator should appear (line start). */
  pos: number;
  /** End position for range indicators (bars). */
  endPos?: number;
  /** Visual type: "dot" for inline marks, "bar" for block ranges. */
  type: "dot" | "bar" | "collapse";
  /** CSS color value or Tailwind bg class. */
  color: string;
  /** Tooltip text shown on hover. */
  tooltip?: string;
  /** Click handler — called when the indicator is tapped/clicked. */
  onClick?: () => void;
  /** Unique key for deduplication. */
  key?: string;
}

// ---------------------------------------------------------------------------
// Plugin Key & State
// ---------------------------------------------------------------------------

export const editorGutterKey = new PluginKey("editorGutter");

/**
 * CSS class applied to the editor wrapper when the gutter is active.
 * Extensions can use this to conditionally style gutter-positioned elements.
 */
export const GUTTER_ACTIVE_CLASS = "editor-gutter-active";

/**
 * Width of the gutter column in pixels.
 * Keep small to minimize layout impact.
 */
export const GUTTER_WIDTH = 20;

// ---------------------------------------------------------------------------
// Gutter indicator rendering utilities
// ---------------------------------------------------------------------------

/**
 * Create a DOM element for a single dot indicator.
 * Position it in the gutter using absolute positioning.
 */
export function createGutterDot(
  color: string,
  tooltip?: string,
  onClick?: () => void,
): HTMLElement {
  const dot = document.createElement("span");
  dot.className = "gutter-indicator gutter-dot";
  dot.setAttribute("aria-hidden", "true");
  dot.setAttribute("contenteditable", "false");

  // Color: either a Tailwind bg class or inline style
  if (color.startsWith("bg-")) {
    dot.classList.add(color);
  } else {
    dot.style.backgroundColor = color;
  }

  if (tooltip) {
    dot.title = tooltip;
  }

  if (onClick) {
    dot.addEventListener("mousedown", (e) => {
      e.preventDefault();
      e.stopPropagation();
      onClick();
    });
    dot.style.cursor = "pointer";
  }

  return dot;
}

/**
 * Create a DOM element that shows multiple colored dots stacked vertically.
 * Used when a single line contains multiple visibility directives with
 * different audience colors.
 */
export function createGutterMultiDot(
  colors: string[],
  tooltip?: string,
  onClick?: () => void,
): HTMLElement {
  if (colors.length === 1) {
    return createGutterDot(colors[0], tooltip, onClick);
  }

  const container = document.createElement("span");
  container.className = "gutter-indicator gutter-multi-dot";
  container.setAttribute("aria-hidden", "true");
  container.setAttribute("contenteditable", "false");

  if (tooltip) {
    container.title = tooltip;
  }

  if (onClick) {
    container.addEventListener("mousedown", (e) => {
      e.preventDefault();
      e.stopPropagation();
      onClick();
    });
    container.style.cursor = "pointer";
  }

  for (const color of colors) {
    const dot = document.createElement("span");
    dot.className = "gutter-multi-dot-segment";
    if (color.startsWith("bg-")) {
      dot.classList.add(color);
    } else {
      dot.style.backgroundColor = color;
    }
    container.appendChild(dot);
  }

  return container;
}

/**
 * Create a small eye SVG icon for the gutter. Used in audience preview mode
 * to replace colored dots with a neutral "peek" affordance.
 */
export function createGutterEyeIcon(
  tooltip?: string,
  onClick?: () => void,
): HTMLElement {
  const wrapper = document.createElement("span");
  wrapper.className = "gutter-indicator gutter-eye";
  wrapper.setAttribute("aria-hidden", "true");
  wrapper.setAttribute("contenteditable", "false");

  // Tiny inline SVG — eye icon at 12×12
  wrapper.innerHTML = `<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0"/><circle cx="12" cy="12" r="3"/></svg>`;

  if (tooltip) {
    wrapper.title = tooltip;
  }

  if (onClick) {
    wrapper.addEventListener("mousedown", (e) => {
      e.preventDefault();
      e.stopPropagation();
      onClick();
    });
    wrapper.style.cursor = "pointer";
  }

  return wrapper;
}

/**
 * Create a DOM element for a collapse indicator (marks where filtered content
 * was removed). Shows a small icon that can be clicked to reveal hidden content.
 */
export function createGutterCollapseMarker(
  color: string,
  tooltip?: string,
  onClick?: () => void,
): HTMLElement {
  const marker = document.createElement("span");
  marker.className = "gutter-indicator gutter-collapse";
  marker.setAttribute("aria-hidden", "true");
  marker.setAttribute("contenteditable", "false");

  if (color.startsWith("bg-")) {
    marker.classList.add(color);
  } else {
    marker.style.backgroundColor = color;
  }

  if (tooltip) {
    marker.title = tooltip;
  }

  if (onClick) {
    marker.addEventListener("mousedown", (e) => {
      e.preventDefault();
      e.stopPropagation();
      onClick();
    });
    marker.style.cursor = "pointer";
  }

  return marker;
}

// ---------------------------------------------------------------------------
// Extension
// ---------------------------------------------------------------------------

export interface EditorGutterOptions {
  /** Whether the gutter is enabled. Default: true. */
  enabled: boolean;
}

/**
 * EditorGutter extension.
 *
 * This extension manages the gutter column CSS. It watches for the presence
 * of gutter-using decorations and toggles the `editor-gutter-active` class
 * on the editor element.
 *
 * Individual extensions (VisibilityMark, VisibilityBlock, etc.) create their
 * own decorations using the gutter CSS utilities. This extension just provides
 * the infrastructure — the space and the CSS.
 */
export const EditorGutter = Extension.create<EditorGutterOptions>({
  name: "editorGutter",

  addOptions() {
    return { enabled: true };
  },

  addProseMirrorPlugins() {
    if (!this.options.enabled) return [];

    return [
      new ProseMirrorPlugin({
        key: editorGutterKey,
        view(editorView) {
          // Track whether gutter class is applied
          let gutterActive = false;

          function updateGutterClass() {
            const el = editorView.dom.closest(".editor-content");
            if (!el) return;

            // Check if any gutter indicators exist in the DOM
            const hasIndicators = el.querySelector(".gutter-indicator") !== null;

            if (hasIndicators && !gutterActive) {
              el.classList.add(GUTTER_ACTIVE_CLASS);
              gutterActive = true;
            } else if (!hasIndicators && gutterActive) {
              el.classList.remove(GUTTER_ACTIVE_CLASS);
              gutterActive = false;
            }
          }

          // Use a MutationObserver to detect when gutter indicators are
          // added/removed. This is more reliable than trying to coordinate
          // across multiple plugins' decoration cycles.
          const observer = new MutationObserver(() => {
            // Debounce slightly to batch decoration updates
            requestAnimationFrame(updateGutterClass);
          });

          observer.observe(editorView.dom, {
            childList: true,
            subtree: true,
          });

          // Initial check
          requestAnimationFrame(updateGutterClass);

          return {
            update() {
              requestAnimationFrame(updateGutterClass);
            },
            destroy() {
              observer.disconnect();
              const el = editorView.dom.closest(".editor-content");
              el?.classList.remove(GUTTER_ACTIVE_CLASS);
            },
          };
        },
      }),
    ];
  },
});
