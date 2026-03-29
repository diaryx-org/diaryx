import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  Puzzle: { name: "Puzzle" },
}));

vi.mock("@lucide/svelte", () => ({
  Puzzle: mocks.Puzzle,
}));

const PuzzleMock = mocks.Puzzle;

import { loadPluginIcon, getCachedPluginIcon } from "./pluginIconResolver";

describe("pluginIconResolver", () => {
  beforeEach(() => {
    vi.resetModules();
  });

  describe("loadPluginIcon", () => {
    it("returns Puzzle fallback for null name", async () => {
      const icon = await loadPluginIcon(null);
      expect(icon).toBe(PuzzleMock);
    });

    it("returns Puzzle fallback for undefined name", async () => {
      const icon = await loadPluginIcon(undefined);
      expect(icon).toBe(PuzzleMock);
    });

    it("returns Puzzle fallback for empty string", async () => {
      const icon = await loadPluginIcon("");
      expect(icon).toBe(PuzzleMock);
    });

    it("returns Puzzle fallback when dynamic import fails", async () => {
      const icon = await loadPluginIcon("nonexistent-icon-xyz-123");
      expect(icon).toBe(PuzzleMock);
    });

    it("caches the Puzzle fallback on import failure", async () => {
      await loadPluginIcon("missing-icon-abc-456");
      const cached = getCachedPluginIcon("missing-icon-abc-456");
      expect(cached).toBe(PuzzleMock);
    });
  });

  describe("getCachedPluginIcon", () => {
    it("returns Puzzle for null name", () => {
      expect(getCachedPluginIcon(null)).toBe(PuzzleMock);
    });

    it("returns Puzzle for undefined name", () => {
      expect(getCachedPluginIcon(undefined)).toBe(PuzzleMock);
    });

    it("returns Puzzle for empty string", () => {
      expect(getCachedPluginIcon("")).toBe(PuzzleMock);
    });

    it("returns Puzzle when icon is not cached", () => {
      expect(getCachedPluginIcon("uncached-icon")).toBe(PuzzleMock);
    });
  });
});
