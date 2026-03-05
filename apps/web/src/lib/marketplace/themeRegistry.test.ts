import { afterEach, describe, expect, it, vi } from "vitest";

import {
  clearThemeRegistryCache,
  fetchThemeRegistry,
  getTrustedThemeRegistryUrls,
} from "./themeRegistry";

const VALID_THEME_REGISTRY_MD = `---
schema_version: 1
generated_at: "2026-03-04T00:00:00Z"
themes:
  - id: "theme.citrus"
    name: "Citrus"
    version: "1.0.0"
    summary: "Fresh yellow and green"
    description: "A bright palette for journaling."
    author: "Diaryx Team"
    license: "PolyForm Shield 1.0.0"
    repository: "https://github.com/diaryx-org/diaryx"
    categories: ["bright"]
    tags: ["yellow", "green"]
    styles: ["playful"]
    icon: null
    screenshots: []
    artifact:
      url: "https://cdn.diaryx.org/themes/artifacts/theme.citrus/1.0.0/theme.json"
      sha256: "abc"
      size: 123
      published_at: "2026-03-04T00:00:00Z"
    theme:
      id: "theme.citrus"
      name: "Citrus"
      version: 1
      colors:
        light:
          background: "oklch(1 0 0)"
          foreground: "oklch(0.1 0.03 260)"
          card: "oklch(1 0 0)"
          card-foreground: "oklch(0.1 0.03 260)"
          popover: "oklch(1 0 0)"
          popover-foreground: "oklch(0.1 0.03 260)"
          primary: "oklch(0.8 0.2 120)"
          primary-foreground: "oklch(0.1 0.03 260)"
          secondary: "oklch(0.95 0.02 110)"
          secondary-foreground: "oklch(0.2 0.04 120)"
          muted: "oklch(0.95 0.02 110)"
          muted-foreground: "oklch(0.5 0.03 120)"
          accent: "oklch(0.95 0.02 110)"
          accent-foreground: "oklch(0.2 0.04 120)"
          destructive: "oklch(0.6 0.2 20)"
          border: "oklch(0.9 0.02 120)"
          input: "oklch(0.9 0.02 120)"
          ring: "oklch(0.7 0.08 120)"
          sidebar: "oklch(0.97 0.01 120)"
          sidebar-foreground: "oklch(0.2 0.04 120)"
          sidebar-primary: "oklch(0.8 0.2 120)"
          sidebar-primary-foreground: "oklch(0.1 0.03 260)"
          sidebar-accent: "oklch(0.95 0.02 110)"
          sidebar-accent-foreground: "oklch(0.2 0.04 120)"
          sidebar-border: "oklch(0.9 0.02 120)"
          sidebar-ring: "oklch(0.7 0.08 120)"
        dark:
          background: "oklch(0.2 0.03 120)"
          foreground: "oklch(0.95 0.01 120)"
          card: "oklch(0.25 0.03 120)"
          card-foreground: "oklch(0.95 0.01 120)"
          popover: "oklch(0.25 0.03 120)"
          popover-foreground: "oklch(0.95 0.01 120)"
          primary: "oklch(0.72 0.16 120)"
          primary-foreground: "oklch(0.2 0.03 120)"
          secondary: "oklch(0.3 0.03 120)"
          secondary-foreground: "oklch(0.95 0.01 120)"
          muted: "oklch(0.3 0.03 120)"
          muted-foreground: "oklch(0.65 0.03 120)"
          accent: "oklch(0.3 0.03 120)"
          accent-foreground: "oklch(0.95 0.01 120)"
          destructive: "oklch(0.65 0.18 20)"
          border: "oklch(0.35 0.03 120)"
          input: "oklch(0.35 0.03 120)"
          ring: "oklch(0.55 0.08 120)"
          sidebar: "oklch(0.22 0.03 120)"
          sidebar-foreground: "oklch(0.95 0.01 120)"
          sidebar-primary: "oklch(0.72 0.16 120)"
          sidebar-primary-foreground: "oklch(0.95 0.01 120)"
          sidebar-accent: "oklch(0.3 0.03 120)"
          sidebar-accent-foreground: "oklch(0.95 0.01 120)"
          sidebar-border: "oklch(0.35 0.03 120)"
          sidebar-ring: "oklch(0.55 0.08 120)"
---
# Theme registry
`;

afterEach(() => {
  clearThemeRegistryCache();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("themeRegistry", () => {
  it("rejects untrusted URLs", async () => {
    await expect(
      fetchThemeRegistry("https://example.com/themes/registry.md"),
    ).rejects.toThrow("Untrusted theme registry URL");
  });

  it("fails on unsupported schema", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: true,
        text: async () => "---\nschema_version: 9\ngenerated_at: \"\"\nthemes: []\n---\n",
      }),
    );

    const trusted = getTrustedThemeRegistryUrls()[0];
    await expect(fetchThemeRegistry(trusted)).rejects.toThrow(
      "Unsupported theme registry schema version",
    );
  });

  it("parses and caches theme registry", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      text: async () => VALID_THEME_REGISTRY_MD,
    });
    vi.stubGlobal("fetch", fetchMock);

    const trusted = getTrustedThemeRegistryUrls()[0];
    const first = await fetchThemeRegistry(trusted);
    const second = await fetchThemeRegistry(trusted);

    expect(first.schema_version).toBe(1);
    expect(first.themes).toHaveLength(1);
    expect(first.themes[0]?.id).toBe("theme.citrus");
    expect(first.themes[0]?.theme.colors.light.primary).toBe("oklch(0.8 0.2 120)");
    expect(second).toBe(first);
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });
});
