import { afterEach, describe, expect, it, vi } from "vitest";

afterEach(() => {
  vi.resetModules();
  vi.restoreAllMocks();
});

describe("resolveCdnBaseUrl", () => {
  it("keeps same-origin /cdn outside Tauri", async () => {
    vi.doMock("$lib/backend/interface", () => ({
      isTauri: () => false,
    }));

    const { resolveCdnBaseUrl } = await import("./cdnBase");
    expect(resolveCdnBaseUrl()).toBe("/cdn");
  });

  it("uses the dev-server origin in Tauri dev", async () => {
    vi.doMock("$lib/backend/interface", () => ({
      isTauri: () => true,
    }));

    const originalLocation = window.location;
    Object.defineProperty(window, "location", {
      configurable: true,
      value: new URL("http://localhost:5174/editor"),
    });

    try {
      const { resolveCdnBaseUrl } = await import("./cdnBase");
      expect(resolveCdnBaseUrl()).toBe("http://localhost:5174/cdn");
    } finally {
      Object.defineProperty(window, "location", {
        configurable: true,
        value: originalLocation,
      });
    }
  });

  it("falls back to the hosted CDN in non-http Tauri shells", async () => {
    vi.doMock("$lib/backend/interface", () => ({
      isTauri: () => true,
    }));

    const originalLocation = window.location;
    Object.defineProperty(window, "location", {
      configurable: true,
      value: new URL("tauri://localhost/index.html"),
    });

    try {
      const { resolveCdnBaseUrl } = await import("./cdnBase");
      expect(resolveCdnBaseUrl()).toBe("https://app.diaryx.org/cdn");
    } finally {
      Object.defineProperty(window, "location", {
        configurable: true,
        value: originalLocation,
      });
    }
  });
});
