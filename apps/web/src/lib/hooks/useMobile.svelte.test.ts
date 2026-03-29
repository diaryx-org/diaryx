import { beforeEach, afterEach, describe, expect, it, vi } from "vitest";

describe("useMobile", () => {
  let originalWindow: typeof globalThis.window | undefined;
  let addedListeners: Record<string, Function[]>;
  let viewportListeners: Record<string, Function[]>;

  function setupWindow(options: {
    innerWidth?: number;
    innerHeight?: number;
    ontouchstart?: boolean;
    maxTouchPoints?: number;
    visualViewport?: { height: number; offsetTop?: number } | null;
    userAgent?: string;
    platform?: string;
  }) {
    addedListeners = {};
    viewportListeners = {};

    const mockVisualViewport = options.visualViewport
      ? {
          height: options.visualViewport.height,
          offsetTop: options.visualViewport.offsetTop ?? 0,
          addEventListener: vi.fn((event: string, handler: Function) => {
            if (!viewportListeners[event]) viewportListeners[event] = [];
            viewportListeners[event].push(handler);
          }),
          removeEventListener: vi.fn(),
        }
      : null;

    const win = {
      innerWidth: options.innerWidth ?? 1024,
      innerHeight: options.innerHeight ?? 768,
      addEventListener: vi.fn((event: string, handler: Function) => {
        if (!addedListeners[event]) addedListeners[event] = [];
        addedListeners[event].push(handler);
      }),
      removeEventListener: vi.fn(),
      visualViewport: mockVisualViewport,
      navigator: {
        maxTouchPoints: options.maxTouchPoints ?? 0,
        userAgent: options.userAgent ?? "Mozilla/5.0",
        platform: options.platform ?? "Win32",
      },
    } as unknown as typeof globalThis.window;

    if (options.ontouchstart) {
      (win as unknown as Record<string, unknown>).ontouchstart = true;
    }

    (globalThis as Record<string, unknown>).window = win;
    (globalThis as Record<string, unknown>).navigator = win.navigator;
  }

  beforeEach(() => {
    originalWindow = (globalThis as { window?: typeof globalThis.window }).window;
    vi.resetModules();
  });

  afterEach(() => {
    (globalThis as { window?: typeof globalThis.window }).window = originalWindow;
  });

  describe("isIOS", () => {
    it("returns false in SSR (no window)", async () => {
      delete (globalThis as Record<string, unknown>).window;
      const { isIOS } = await import("./useMobile.svelte");
      expect(isIOS()).toBe(false);
    });

    it("detects iPhone user agent", async () => {
      setupWindow({ userAgent: "Mozilla/5.0 (iPhone; CPU iPhone OS)" });
      const { isIOS } = await import("./useMobile.svelte");
      expect(isIOS()).toBe(true);
    });

    it("detects iPad user agent", async () => {
      setupWindow({ userAgent: "Mozilla/5.0 (iPad; CPU OS)" });
      const { isIOS } = await import("./useMobile.svelte");
      expect(isIOS()).toBe(true);
    });

    it("detects iPod user agent", async () => {
      setupWindow({ userAgent: "Mozilla/5.0 (iPod touch)" });
      const { isIOS } = await import("./useMobile.svelte");
      expect(isIOS()).toBe(true);
    });

    it("detects iPad on macOS (MacIntel + touch)", async () => {
      setupWindow({ platform: "MacIntel", maxTouchPoints: 5 });
      const { isIOS } = await import("./useMobile.svelte");
      expect(isIOS()).toBe(true);
    });

    it("returns false for MacIntel without touch", async () => {
      setupWindow({ platform: "MacIntel", maxTouchPoints: 0 });
      const { isIOS } = await import("./useMobile.svelte");
      expect(isIOS()).toBe(false);
    });

    it("returns false for Android", async () => {
      setupWindow({ userAgent: "Mozilla/5.0 (Linux; Android 13)" });
      const { isIOS } = await import("./useMobile.svelte");
      expect(isIOS()).toBe(false);
    });
  });

  describe("isAndroid", () => {
    it("returns false in SSR (no window)", async () => {
      delete (globalThis as Record<string, unknown>).window;
      const { isAndroid } = await import("./useMobile.svelte");
      expect(isAndroid()).toBe(false);
    });

    it("detects Android user agent", async () => {
      setupWindow({ userAgent: "Mozilla/5.0 (Linux; Android 13)" });
      const { isAndroid } = await import("./useMobile.svelte");
      expect(isAndroid()).toBe(true);
    });

    it("returns false for iOS", async () => {
      setupWindow({ userAgent: "Mozilla/5.0 (iPhone; CPU iPhone OS)" });
      const { isAndroid } = await import("./useMobile.svelte");
      expect(isAndroid()).toBe(false);
    });
  });

  describe("getMobileState", () => {
    it("returns SSR fallback when window is undefined", async () => {
      delete (globalThis as Record<string, unknown>).window;
      const { getMobileState } = await import("./useMobile.svelte");
      const state = getMobileState();
      expect(state.isMobile).toBe(false);
      expect(state.isTouchDevice).toBe(false);
      expect(state.keyboardVisible).toBe(false);
      expect(state.keyboardHeight).toBe(0);
      expect(state.viewportOffsetTop).toBe(0);
      expect(state.viewportHeight).toBe(0);
    });
  });

  describe("createMobileState", () => {
    it("detects mobile when width < 768", async () => {
      setupWindow({ innerWidth: 400, innerHeight: 800 });
      const { createMobileState } = await import("./useMobile.svelte");
      const state = createMobileState();
      expect(state.isMobile).toBe(true);
    });

    it("detects non-mobile when width >= 768", async () => {
      setupWindow({ innerWidth: 1024, innerHeight: 768 });
      const { createMobileState } = await import("./useMobile.svelte");
      const state = createMobileState();
      expect(state.isMobile).toBe(false);
    });

    it("detects touch device via ontouchstart", async () => {
      setupWindow({ ontouchstart: true });
      const { createMobileState } = await import("./useMobile.svelte");
      const state = createMobileState();
      expect(state.isTouchDevice).toBe(true);
    });

    it("detects touch device via maxTouchPoints", async () => {
      setupWindow({ maxTouchPoints: 5 });
      const { createMobileState } = await import("./useMobile.svelte");
      const state = createMobileState();
      expect(state.isTouchDevice).toBe(true);
    });

    it("detects non-touch device", async () => {
      setupWindow({ maxTouchPoints: 0 });
      const { createMobileState } = await import("./useMobile.svelte");
      const state = createMobileState();
      expect(state.isTouchDevice).toBe(false);
    });

    it("sets initial viewportHeight from window.innerHeight", async () => {
      setupWindow({ innerHeight: 900 });
      const { createMobileState } = await import("./useMobile.svelte");
      const state = createMobileState();
      expect(state.viewportHeight).toBe(900);
    });

    it("registers resize listener", async () => {
      setupWindow({});
      const { createMobileState } = await import("./useMobile.svelte");
      createMobileState();
      expect(addedListeners["resize"]).toBeDefined();
      expect(addedListeners["resize"].length).toBe(1);
    });

    it("registers visualViewport listeners when available", async () => {
      setupWindow({
        innerHeight: 800,
        visualViewport: { height: 800, offsetTop: 0 },
      });
      const { createMobileState } = await import("./useMobile.svelte");
      createMobileState();
      expect(viewportListeners["resize"]).toBeDefined();
      expect(viewportListeners["scroll"]).toBeDefined();
    });

    it("detects keyboard visible when viewport height diff > 150", async () => {
      setupWindow({
        innerHeight: 800,
        visualViewport: { height: 400, offsetTop: 0 },
      });
      const { createMobileState } = await import("./useMobile.svelte");
      const state = createMobileState();
      expect(state.keyboardVisible).toBe(true);
      expect(state.keyboardHeight).toBe(400);
    });

    it("does not detect keyboard when viewport diff < 150", async () => {
      setupWindow({
        innerHeight: 800,
        visualViewport: { height: 700, offsetTop: 0 },
      });
      const { createMobileState } = await import("./useMobile.svelte");
      const state = createMobileState();
      expect(state.keyboardVisible).toBe(false);
      expect(state.keyboardHeight).toBe(0);
    });
  });
});
