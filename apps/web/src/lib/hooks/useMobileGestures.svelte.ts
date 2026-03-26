/**
 * Mobile gesture hook — tracks touch swipe gestures for opening/closing
 * sidebars and the command palette.
 *
 * Returns reactive state (swipe progress values) and lifecycle methods
 * (attach / cleanup) for use in the top-level App component.
 *
 * Extracted from App.svelte to keep the main component focused on UI wiring.
 */

import {
  getMobileSwipeStartContext,
  hasNonCollapsedSelection,
} from "$lib/mobileSwipe";
import { uiStore } from "../../models/stores";

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const SWIPE_LOCK_PX = 15;
const SWIPE_COMMIT_FRACTION = 0.35;
const COMMAND_PALETTE_TRAVEL_PX = 320;
const COMMAND_PALETTE_EDGE_ZONE_PX = 100;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

type SwipeTarget =
  | "open-left"
  | "close-left"
  | "open-right"
  | "close-right"
  | "open-command-palette"
  | null;

// ---------------------------------------------------------------------------
// Hook
// ---------------------------------------------------------------------------

export function useMobileGestures() {
  // Internal (non-reactive) tracking state
  let touchStartX = 0;
  let touchStartY = 0;
  let trackingTouchGesture = false;
  let touchBlocksShellSwipe = false;
  let touchStartedInSelectableContent = false;
  let touchStartTarget: EventTarget | null = null;
  let cleanupFn: (() => void) | null = null;

  // Reactive state consumed by the template
  let editorFabElement: HTMLElement | null = $state(null);
  let swipeTarget: SwipeTarget = $state(null);
  let swipeProgress = $state(0);

  let leftSidebarSwipeProgress: number | null = $derived(
    swipeTarget === "open-left" || swipeTarget === "close-left" ? swipeProgress : null,
  );
  let rightSidebarSwipeProgress: number | null = $derived(
    swipeTarget === "open-right" || swipeTarget === "close-right" ? swipeProgress : null,
  );
  let commandPaletteSwipeProgress: number | null = $derived(
    swipeTarget === "open-command-palette" ? swipeProgress : null,
  );

  // -----------------------------------------------------------------------
  // Helpers
  // -----------------------------------------------------------------------

  function reset() {
    touchStartX = 0;
    touchStartY = 0;
    trackingTouchGesture = false;
    touchBlocksShellSwipe = false;
    touchStartedInSelectableContent = false;
    touchStartTarget = null;
    swipeTarget = null;
    swipeProgress = 0;
  }

  function resolveSwipeTarget(deltaX: number): SwipeTarget {
    const leftCollapsed = uiStore.leftSidebarCollapsed;
    const rightCollapsed = uiStore.rightSidebarCollapsed;

    if (deltaX > 0) {
      if (!rightCollapsed) return "close-right";
      if (leftCollapsed) return "open-left";
    } else {
      if (!leftCollapsed) return "close-left";
      if (rightCollapsed) return "open-right";
    }
    return null;
  }

  // -----------------------------------------------------------------------
  // Touch event handlers
  // -----------------------------------------------------------------------

  function handleTouchStart(e: TouchEvent) {
    if (e.touches.length !== 1) {
      reset();
      return;
    }

    const touch = e.touches[0];
    const swipeContext = getMobileSwipeStartContext(e.target);

    touchStartX = touch.clientX;
    touchStartY = touch.clientY;
    trackingTouchGesture = true;
    touchBlocksShellSwipe = swipeContext.blocksShellSwipe;
    touchStartedInSelectableContent = swipeContext.startsInSelectableContent;
    touchStartTarget = e.target;
    swipeTarget = null;
    swipeProgress = 0;
  }

  function handleTouchMove(e: TouchEvent) {
    if (!trackingTouchGesture || e.touches.length !== 1) return;
    if (touchBlocksShellSwipe) return;

    const touch = e.touches[0];
    const deltaX = touch.clientX - touchStartX;
    const deltaY = touch.clientY - touchStartY;
    const absDeltaX = Math.abs(deltaX);
    const absDeltaY = Math.abs(deltaY);

    // If the direction isn't locked yet, try to lock it
    if (!swipeTarget) {
      if (absDeltaX < SWIPE_LOCK_PX && absDeltaY < SWIPE_LOCK_PX) return;

      // Check for active text selection before locking
      if (touchStartedInSelectableContent) {
        const selection =
          typeof window.getSelection === "function" ? window.getSelection() : null;
        if (hasNonCollapsedSelection(selection)) return;
      }

      if (absDeltaY > absDeltaX) {
        // Mostly vertical swipe-up → open command palette
        const viewportHeight = window.innerHeight;
        const startedOnFab = editorFabElement
          && touchStartTarget instanceof Node
          && editorFabElement.contains(touchStartTarget);
        const startedInBottomZone = touchStartY > viewportHeight - COMMAND_PALETTE_EDGE_ZONE_PX;
        if (
          deltaY < 0 &&
          (startedOnFab || (!editorFabElement && startedInBottomZone))
        ) {
          swipeTarget = "open-command-palette";
        } else {
          return; // vertical but not from FAB/footer – ignore
        }
      } else {
        swipeTarget = resolveSwipeTarget(deltaX);
        if (!swipeTarget) return;
      }
    }

    // Compute progress (0–1) based on the swipe target
    const leftWidth = uiStore.leftSidebarWidth;
    const rightWidth = uiStore.rightSidebarWidth;
    let raw: number;
    switch (swipeTarget) {
      case "open-left":
        raw = deltaX / leftWidth;
        break;
      case "close-left":
        raw = 1 + deltaX / leftWidth;
        break;
      case "open-right":
        raw = -deltaX / rightWidth;
        break;
      case "close-right":
        raw = 1 - deltaX / rightWidth;
        break;
      case "open-command-palette":
        raw = -deltaY / COMMAND_PALETTE_TRAVEL_PX;
        break;
      default:
        return;
    }

    swipeProgress = Math.max(0, Math.min(1, raw));
  }

  function handleTouchEnd(e: TouchEvent) {
    if (!trackingTouchGesture || e.changedTouches.length === 0) {
      reset();
      return;
    }

    if (swipeTarget) {
      const commit = swipeProgress >= SWIPE_COMMIT_FRACTION;
      switch (swipeTarget) {
        case "open-left":
          if (commit) uiStore.setLeftSidebarCollapsed(false);
          break;
        case "close-left":
          if (swipeProgress < (1 - SWIPE_COMMIT_FRACTION)) uiStore.setLeftSidebarCollapsed(true);
          break;
        case "open-right":
          if (commit) uiStore.setRightSidebarCollapsed(false);
          break;
        case "close-right":
          if (swipeProgress < (1 - SWIPE_COMMIT_FRACTION)) uiStore.setRightSidebarCollapsed(true);
          break;
        case "open-command-palette":
          if (commit) uiStore.openCommandPalette();
          break;
      }
      reset();
      return;
    }

    reset();
  }

  // -----------------------------------------------------------------------
  // Lifecycle
  // -----------------------------------------------------------------------

  function attach() {
    if (cleanupFn) return;

    document.addEventListener("touchstart", handleTouchStart, { passive: true });
    document.addEventListener("touchmove", handleTouchMove, { passive: true });
    document.addEventListener("touchend", handleTouchEnd, { passive: true });
    document.addEventListener("touchcancel", reset, { passive: true });

    cleanupFn = () => {
      document.removeEventListener("touchstart", handleTouchStart);
      document.removeEventListener("touchmove", handleTouchMove);
      document.removeEventListener("touchend", handleTouchEnd);
      document.removeEventListener("touchcancel", reset);
      cleanupFn = null;
    };
  }

  function cleanup() {
    cleanupFn?.();
  }

  // -----------------------------------------------------------------------
  // Public API
  // -----------------------------------------------------------------------

  return {
    get leftSidebarSwipeProgress() { return leftSidebarSwipeProgress; },
    get rightSidebarSwipeProgress() { return rightSidebarSwipeProgress; },
    get commandPaletteSwipeProgress() { return commandPaletteSwipeProgress; },
    get editorFabElement() { return editorFabElement; },
    set editorFabElement(el: HTMLElement | null) { editorFabElement = el; },
    attach,
    cleanup,
  };
}
