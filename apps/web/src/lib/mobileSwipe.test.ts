import { describe, expect, it } from "vitest";

import {
  MOBILE_SWIPE_EDGE_ZONE_PX,
  getMobileSwipeStartContext,
  hasNonCollapsedSelection,
  isSidebarSwipeEdgeStart,
} from "./mobileSwipe";

describe("mobileSwipe", () => {
  it("blocks shell gestures inside dialog content", () => {
    document.body.innerHTML = `
      <div data-slot="dialog-content">
        <button id="target" type="button">Open</button>
      </div>
    `;

    const target = document.getElementById("target");
    expect(target).not.toBeNull();
    expect(getMobileSwipeStartContext(target).blocksShellSwipe).toBe(true);
  });

  it("blocks shell gestures inside drawer content", () => {
    document.body.innerHTML = `
      <div data-slot="drawer-content">
        <div id="target">Panel</div>
      </div>
    `;

    const target = document.getElementById("target");
    expect(target).not.toBeNull();
    expect(getMobileSwipeStartContext(target).blocksShellSwipe).toBe(true);
  });

  it("treats editor content as selectable", () => {
    document.body.innerHTML = `
      <div class="editor-content">
        <p id="target">Hello</p>
      </div>
    `;

    const target = document.getElementById("target");
    expect(target).not.toBeNull();
    expect(getMobileSwipeStartContext(target).startsInSelectableContent).toBe(
      true,
    );
  });

  it("treats form fields as selectable", () => {
    document.body.innerHTML = `<input id="target" type="text" value="hello" />`;

    const target = document.getElementById("target");
    expect(target).not.toBeNull();
    expect(getMobileSwipeStartContext(target).startsInSelectableContent).toBe(
      true,
    );
  });

  it("detects non-collapsed selections", () => {
    expect(hasNonCollapsedSelection({ isCollapsed: false })).toBe(true);
    expect(hasNonCollapsedSelection({ isCollapsed: true })).toBe(false);
    expect(hasNonCollapsedSelection(null)).toBe(false);
  });

  it("only allows sidebar opens from the matching screen edge", () => {
    const viewportWidth = 390;

    expect(
      isSidebarSwipeEdgeStart(
        MOBILE_SWIPE_EDGE_ZONE_PX,
        viewportWidth,
        "right",
      ),
    ).toBe(true);
    expect(
      isSidebarSwipeEdgeStart(
        viewportWidth - MOBILE_SWIPE_EDGE_ZONE_PX,
        viewportWidth,
        "left",
      ),
    ).toBe(true);
    expect(isSidebarSwipeEdgeStart(120, viewportWidth, "right")).toBe(false);
    expect(isSidebarSwipeEdgeStart(120, viewportWidth, "left")).toBe(false);
  });
});
