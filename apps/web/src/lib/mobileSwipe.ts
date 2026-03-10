const SHELL_GESTURE_EXCLUSION_SELECTOR = [
  '[data-slot="dialog-content"]',
  '[data-slot="dialog-overlay"]',
  '[data-slot="drawer-content"]',
  '[data-slot="drawer-overlay"]',
  '[aria-modal="true"]',
].join(", ");

const SELECTABLE_CONTENT_SELECTOR = [
  ".editor-content",
  "textarea",
  "select",
  '[contenteditable=""]',
  '[contenteditable="true"]',
  '[contenteditable="plaintext-only"]',
  '[role="textbox"]',
  'input:not([type="button"]):not([type="checkbox"]):not([type="radio"]):not([type="submit"]):not([type="reset"])',
].join(", ");

export const MOBILE_SWIPE_EDGE_ZONE_PX = 32;

export interface MobileSwipeStartContext {
  blocksShellSwipe: boolean;
  startsInSelectableContent: boolean;
}

function toElement(target: EventTarget | null): Element | null {
  if (typeof Element !== "undefined" && target instanceof Element) {
    return target;
  }
  if (typeof Node !== "undefined" && target instanceof Node) {
    return target.parentElement;
  }
  return null;
}

export function getMobileSwipeStartContext(
  target: EventTarget | null,
): MobileSwipeStartContext {
  const element = toElement(target);
  if (!element) {
    return {
      blocksShellSwipe: false,
      startsInSelectableContent: false,
    };
  }

  return {
    blocksShellSwipe: Boolean(
      element.closest(SHELL_GESTURE_EXCLUSION_SELECTOR),
    ),
    startsInSelectableContent: Boolean(
      element.closest(SELECTABLE_CONTENT_SELECTOR),
    ),
  };
}

export function hasNonCollapsedSelection(
  selection: Pick<Selection, "isCollapsed"> | null | undefined,
): boolean {
  return Boolean(selection && !selection.isCollapsed);
}

export function isSidebarSwipeEdgeStart(
  touchStartX: number,
  viewportWidth: number,
  direction: "left" | "right",
): boolean {
  if (
    !Number.isFinite(touchStartX) ||
    !Number.isFinite(viewportWidth) ||
    viewportWidth <= 0
  ) {
    return false;
  }

  if (direction === "right") {
    return touchStartX <= MOBILE_SWIPE_EDGE_ZONE_PX;
  }

  return touchStartX >= viewportWidth - MOBILE_SWIPE_EDGE_ZONE_PX;
}
