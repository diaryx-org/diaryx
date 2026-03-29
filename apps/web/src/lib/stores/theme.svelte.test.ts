import { beforeEach, describe, expect, it, vi } from "vitest";

// ---------------------------------------------------------------------------
// Mock workspace persistence
// ---------------------------------------------------------------------------

vi.mock("$lib/workspace/workspaceAssetStorage", () => ({
  readWorkspaceText: vi.fn().mockResolvedValue(null),
  writeWorkspaceText: vi.fn().mockResolvedValue(undefined),
  getThemeModePath: vi.fn().mockReturnValue(".diaryx/themes/mode.json"),
  getThemeSettingsPath: vi.fn().mockReturnValue(".diaryx/themes/settings.json"),
  getThemeLibraryPath: vi.fn().mockReturnValue(".diaryx/themes/library.json"),
  getTypographySettingsPath: vi.fn().mockReturnValue(".diaryx/typographies/settings.json"),
  getTypographyLibraryPath: vi.fn().mockReturnValue(".diaryx/typographies/library.json"),
}));

import { createThemeStore, getThemeStore } from "./theme.svelte";
import {
  readWorkspaceText,
  writeWorkspaceText,
} from "$lib/workspace/workspaceAssetStorage";

beforeEach(() => {
  localStorage.clear();
  document.documentElement.classList.remove("dark");

  if (!window.matchMedia) {
    Object.defineProperty(window, "matchMedia", {
      writable: true,
      value: vi.fn().mockImplementation(() => ({
        matches: false,
        media: "",
        onchange: null,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
        dispatchEvent: vi.fn(),
      })),
    });
  }
});

// ===========================================================================
// createThemeStore
// ===========================================================================

describe("createThemeStore", () => {
  it("initializes with system mode", () => {
    const store = createThemeStore();
    expect(store.mode).toBe("system");
  });

  it("resolves system mode to light when prefers-color-scheme is light", () => {
    const store = createThemeStore();
    // matchMedia mock returns matches: false (light)
    expect(store.resolvedTheme).toBe("light");
    expect(store.isDark).toBe(false);
  });

  it("resolves system mode to dark when prefers-color-scheme is dark", () => {
    Object.defineProperty(window, "matchMedia", {
      writable: true,
      value: vi.fn().mockImplementation(() => ({
        matches: true,
        media: "(prefers-color-scheme: dark)",
        onchange: null,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
        dispatchEvent: vi.fn(),
      })),
    });

    const store = createThemeStore();
    expect(store.resolvedTheme).toBe("dark");
    expect(store.isDark).toBe(true);
  });

  it("setMode changes mode and persists to localStorage", () => {
    const store = createThemeStore();
    store.setMode("dark");
    expect(store.mode).toBe("dark");
    expect(store.resolvedTheme).toBe("dark");
    expect(localStorage.getItem("diaryx-theme")).toBe("dark");
  });

  it("setMode to light works", () => {
    const store = createThemeStore();
    store.setMode("light");
    expect(store.mode).toBe("light");
    expect(store.resolvedTheme).toBe("light");
    expect(store.isDark).toBe(false);
    expect(localStorage.getItem("diaryx-theme")).toBe("light");
  });

  it("toggle switches between light and dark", () => {
    const store = createThemeStore();
    // Start from an explicit light mode to avoid depending on matchMedia mock state
    store.setMode("light");
    expect(store.resolvedTheme).toBe("light");

    store.toggle();
    expect(store.mode).toBe("dark");
    expect(store.resolvedTheme).toBe("dark");

    store.toggle();
    expect(store.mode).toBe("light");
    expect(store.resolvedTheme).toBe("light");
  });

  it("loads persisted mode from localStorage", () => {
    localStorage.setItem("diaryx-theme", "dark");
    const store = createThemeStore();
    expect(store.mode).toBe("dark");
    expect(store.resolvedTheme).toBe("dark");
  });

  it("ignores invalid localStorage value", () => {
    localStorage.setItem("diaryx-theme", "invalid-mode");
    const store = createThemeStore();
    expect(store.mode).toBe("system");
  });

  it("adds dark class to document when dark", () => {
    const store = createThemeStore();
    store.setMode("dark");
    expect(document.documentElement.classList.contains("dark")).toBe(true);

    store.setMode("light");
    expect(document.documentElement.classList.contains("dark")).toBe(false);
  });

  it("onModeChange registers and invokes listeners", () => {
    const store = createThemeStore();
    const listener = vi.fn();
    const unsubscribe = store.onModeChange(listener);

    store.setMode("dark");
    expect(listener).toHaveBeenCalledTimes(1);

    store.setMode("light");
    expect(listener).toHaveBeenCalledTimes(2);

    unsubscribe();
    store.setMode("dark");
    // Should not be called again after unsubscribe
    expect(listener).toHaveBeenCalledTimes(2);
  });

  it("hydrateThemeMode sets mode from workspace config", () => {
    const store = createThemeStore();
    const persistFn = vi.fn().mockResolvedValue(undefined);

    store.hydrateThemeMode("dark", persistFn);
    expect(store.mode).toBe("dark");
    expect(store.resolvedTheme).toBe("dark");
  });

  it("hydrateThemeMode ignores invalid mode string", () => {
    const store = createThemeStore();
    const persistFn = vi.fn().mockResolvedValue(undefined);

    store.hydrateThemeMode("invalid", persistFn);
    // Should stay as system (default)
    expect(store.mode).toBe("system");
  });

  it("hydrateThemeMode wires up persistFn for future setMode calls", () => {
    const store = createThemeStore();
    const persistFn = vi.fn().mockResolvedValue(undefined);

    store.hydrateThemeMode(undefined, persistFn);
    // persistFn not called yet (no valid mode to hydrate)
    store.setMode("dark");
    expect(persistFn).toHaveBeenCalledWith("dark");
  });

  it("reloadFromWorkspace reads and applies workspace theme mode", async () => {
    vi.mocked(readWorkspaceText).mockResolvedValueOnce(
      JSON.stringify({ mode: "dark" }),
    );

    const store = createThemeStore();
    await store.reloadFromWorkspace();
    expect(store.mode).toBe("dark");
  });

  it("reloadFromWorkspace persists defaults when no workspace file exists", async () => {
    vi.mocked(readWorkspaceText).mockResolvedValueOnce(null);

    const store = createThemeStore();
    await store.reloadFromWorkspace();
    // Should remain at default
    expect(store.mode).toBe("system");
    // Should have persisted current mode to workspace
    expect(writeWorkspaceText).toHaveBeenCalled();
  });

  it("reloadFromWorkspace handles parse errors gracefully", async () => {
    vi.mocked(readWorkspaceText).mockResolvedValueOnce("not-json!!!");

    const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
    const store = createThemeStore();
    await store.reloadFromWorkspace();
    expect(store.mode).toBe("system");
    warnSpy.mockRestore();
  });

  it("reloadFromWorkspace ignores invalid mode in file", async () => {
    vi.mocked(readWorkspaceText).mockResolvedValueOnce(
      JSON.stringify({ mode: "banana" }),
    );

    const store = createThemeStore();
    await store.reloadFromWorkspace();
    // Should stay at system since "banana" is not valid
    expect(store.mode).toBe("system");
  });
});

// ===========================================================================
// getThemeStore singleton
// ===========================================================================

describe("getThemeStore", () => {
  it("returns a store object with expected shape", () => {
    const store = getThemeStore();
    expect(store.mode).toBeDefined();
    expect(store.resolvedTheme).toBeDefined();
    expect(typeof store.isDark).toBe("boolean");
    expect(typeof store.setMode).toBe("function");
    expect(typeof store.toggle).toBe("function");
    expect(typeof store.reloadFromWorkspace).toBe("function");
    expect(typeof store.hydrateThemeMode).toBe("function");
    expect(typeof store.onModeChange).toBe("function");
  });
});
