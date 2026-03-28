import type { SpotlightPlacement } from "$lib/marketplace/types";

export const SPOTLIGHT_PAD = 8;
export const SPOTLIGHT_RADIUS = 8;
export const SPOTLIGHT_TOOLTIP_GAP = 12;
export const SPOTLIGHT_TOOLTIP_WIDTH = 320;
export const SPOTLIGHT_TOOLTIP_HEIGHT_ESTIMATE = 140;
export const SPOTLIGHT_VIEWPORT_MARGIN = 16;
export const SPOTLIGHT_MIN_SWIPE_DISTANCE = 50;

export interface RectLike {
  left: number;
  top: number;
  right: number;
  bottom: number;
  width: number;
  height: number;
}

export interface SpotlightCutout {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface SpotlightViewport {
  width: number;
  height: number;
}

export type SpotlightSwipeDirection = "next" | "previous" | null;

export function getSpotlightCutout(rect: RectLike | null, pad = SPOTLIGHT_PAD): SpotlightCutout {
  if (!rect) {
    return { x: 0, y: 0, width: 0, height: 0 };
  }

  return {
    x: rect.left - pad,
    y: rect.top - pad,
    width: rect.width + pad * 2,
    height: rect.height + pad * 2,
  };
}

export function getSpotlightSwipeDirection(
  dx: number,
  dy: number,
  minDistance = SPOTLIGHT_MIN_SWIPE_DISTANCE,
): SpotlightSwipeDirection {
  if (Math.abs(dx) <= minDistance || Math.abs(dx) <= Math.abs(dy)) {
    return null;
  }

  return dx < 0 ? "next" : "previous";
}

export function shouldPlaceMobileCardAtTop(
  isMobileMode: boolean,
  rect: RectLike | null,
  viewportHeight: number,
): boolean {
  return Boolean(isMobileMode && rect && rect.bottom > viewportHeight / 2);
}

export function shouldAdvanceWhenTargetCollapses(isMobileMode: boolean, rect: RectLike): boolean {
  return isMobileMode && rect.width === 0 && rect.height === 0;
}

export function getSpotlightTooltipStyle(
  rect: RectLike | null,
  placement: SpotlightPlacement | null,
  isMobileMode: boolean,
  viewport: SpotlightViewport,
): string {
  if (!rect || !placement || isMobileMode) {
    return "display: none";
  }

  const { width: viewportWidth, height: viewportHeight } = viewport;
  let top = 0;
  let left = 0;

  switch (placement) {
    case "right":
      top = rect.top + rect.height / 2 - SPOTLIGHT_TOOLTIP_HEIGHT_ESTIMATE / 2;
      left = rect.right + SPOTLIGHT_PAD + SPOTLIGHT_TOOLTIP_GAP;
      break;
    case "left":
      top = rect.top + rect.height / 2 - SPOTLIGHT_TOOLTIP_HEIGHT_ESTIMATE / 2;
      left = rect.left - SPOTLIGHT_PAD - SPOTLIGHT_TOOLTIP_GAP - SPOTLIGHT_TOOLTIP_WIDTH;
      break;
    case "bottom":
      top = rect.bottom + SPOTLIGHT_PAD + SPOTLIGHT_TOOLTIP_GAP;
      left = rect.left + rect.width / 2 - SPOTLIGHT_TOOLTIP_WIDTH / 2;
      break;
    case "top":
      top = rect.top - SPOTLIGHT_PAD - SPOTLIGHT_TOOLTIP_GAP - SPOTLIGHT_TOOLTIP_HEIGHT_ESTIMATE;
      left = rect.left + rect.width / 2 - SPOTLIGHT_TOOLTIP_WIDTH / 2;
      break;
    default:
      return "display: none";
  }

  top = Math.max(
    SPOTLIGHT_VIEWPORT_MARGIN,
    Math.min(top, viewportHeight - SPOTLIGHT_TOOLTIP_HEIGHT_ESTIMATE - SPOTLIGHT_VIEWPORT_MARGIN),
  );
  left = Math.max(
    SPOTLIGHT_VIEWPORT_MARGIN,
    Math.min(left, viewportWidth - SPOTLIGHT_TOOLTIP_WIDTH - SPOTLIGHT_VIEWPORT_MARGIN),
  );

  return `top: ${top}px; left: ${left}px; width: ${SPOTLIGHT_TOOLTIP_WIDTH}px`;
}
