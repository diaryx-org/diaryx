import { afterEach, describe, expect, it } from "vitest";

import { isBrowser, isTauri } from "./interface";

const originalIsTauri = (globalThis as any).isTauri;
const originalTauriInternals = (globalThis as any).__TAURI_INTERNALS__;

afterEach(() => {
  if (originalIsTauri === undefined) {
    delete (globalThis as any).isTauri;
  } else {
    (globalThis as any).isTauri = originalIsTauri;
  }

  if (originalTauriInternals === undefined) {
    delete (globalThis as any).__TAURI_INTERNALS__;
  } else {
    (globalThis as any).__TAURI_INTERNALS__ = originalTauriInternals;
  }
});

describe("runtime detection", () => {
  it("detects Tauri from the Tauri v2 isTauri marker", () => {
    (globalThis as any).isTauri = true;
    delete (globalThis as any).__TAURI_INTERNALS__;

    expect(isTauri()).toBe(true);
    expect(isBrowser()).toBe(false);
  });

  it("detects Tauri from Tauri internals when the global marker is absent", () => {
    delete (globalThis as any).isTauri;
    (globalThis as any).__TAURI_INTERNALS__ = {};

    expect(isTauri()).toBe(true);
    expect(isBrowser()).toBe(false);
  });

  it("falls back to browser mode when no Tauri runtime markers exist", () => {
    delete (globalThis as any).isTauri;
    delete (globalThis as any).__TAURI_INTERNALS__;

    expect(isTauri()).toBe(false);
    expect(isBrowser()).toBe(true);
  });
});
