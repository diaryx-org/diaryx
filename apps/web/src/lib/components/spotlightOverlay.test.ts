import { describe, expect, it } from "vitest";

import {
  getSpotlightCutout,
  getSpotlightSwipeDirection,
  getSpotlightTooltipStyle,
  shouldAdvanceWhenTargetCollapses,
  shouldPlaceMobileCardAtTop,
} from "./spotlightOverlay";

const rect = {
  left: 100,
  top: 120,
  right: 220,
  bottom: 180,
  width: 120,
  height: 60,
};

describe("spotlightOverlay", () => {
  it("expands the spotlight cutout with padding", () => {
    expect(getSpotlightCutout(rect)).toEqual({
      x: 92,
      y: 112,
      width: 136,
      height: 76,
    });
  });

  it("returns a hidden tooltip style when tooltip rendering is disabled", () => {
    expect(getSpotlightTooltipStyle(null, "right", false, { width: 1280, height: 720 })).toBe(
      "display: none",
    );
    expect(getSpotlightTooltipStyle(rect, "right", true, { width: 1280, height: 720 })).toBe(
      "display: none",
    );
  });

  it("positions and clamps tooltip styles inside the viewport", () => {
    expect(getSpotlightTooltipStyle(rect, "right", false, { width: 1280, height: 720 })).toBe(
      "top: 80px; left: 240px; width: 320px",
    );

    expect(
      getSpotlightTooltipStyle(
        {
          left: 10,
          top: 20,
          right: 40,
          bottom: 60,
          width: 30,
          height: 40,
        },
        "left",
        false,
        { width: 400, height: 220 },
      ),
    ).toBe("top: 16px; left: 16px; width: 320px");
  });

  it("interprets only strong horizontal swipes as navigation", () => {
    expect(getSpotlightSwipeDirection(-80, 20)).toBe("next");
    expect(getSpotlightSwipeDirection(90, 10)).toBe("previous");
    expect(getSpotlightSwipeDirection(40, 5)).toBeNull();
    expect(getSpotlightSwipeDirection(80, 120)).toBeNull();
  });

  it("detects mobile card placement and collapsed targets", () => {
    expect(shouldPlaceMobileCardAtTop(true, rect, 300)).toBe(true);
    expect(shouldPlaceMobileCardAtTop(true, rect, 500)).toBe(false);
    expect(shouldPlaceMobileCardAtTop(false, rect, 300)).toBe(false);

    expect(shouldAdvanceWhenTargetCollapses(true, { ...rect, width: 0, height: 0 })).toBe(true);
    expect(shouldAdvanceWhenTargetCollapses(false, { ...rect, width: 0, height: 0 })).toBe(false);
  });
});
