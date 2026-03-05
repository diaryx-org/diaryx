import { describe, expect, it } from "vitest";

import type { RegistryPlugin } from "$lib/plugins/pluginRegistry";
import type {
  BundleRegistryEntry,
  ThemeRegistryEntry,
  TypographyRegistryEntry,
} from "$lib/marketplace/types";
import {
  executeBundleApply,
  planBundleApply,
  type BundleApplyRuntime,
} from "./bundleApply";

function palette(seed: string) {
  return {
    background: `${seed}-bg`,
    foreground: `${seed}-fg`,
    card: `${seed}-card`,
    "card-foreground": `${seed}-card-fg`,
    popover: `${seed}-popover`,
    "popover-foreground": `${seed}-popover-fg`,
    primary: `${seed}-primary`,
    "primary-foreground": `${seed}-primary-fg`,
    secondary: `${seed}-secondary`,
    "secondary-foreground": `${seed}-secondary-fg`,
    muted: `${seed}-muted`,
    "muted-foreground": `${seed}-muted-fg`,
    accent: `${seed}-accent`,
    "accent-foreground": `${seed}-accent-fg`,
    destructive: `${seed}-destructive`,
    border: `${seed}-border`,
    input: `${seed}-input`,
    ring: `${seed}-ring`,
    sidebar: `${seed}-sidebar`,
    "sidebar-foreground": `${seed}-sidebar-fg`,
    "sidebar-primary": `${seed}-sidebar-primary`,
    "sidebar-primary-foreground": `${seed}-sidebar-primary-fg`,
    "sidebar-accent": `${seed}-sidebar-accent`,
    "sidebar-accent-foreground": `${seed}-sidebar-accent-fg`,
    "sidebar-border": `${seed}-sidebar-border`,
    "sidebar-ring": `${seed}-sidebar-ring`,
  };
}

function createThemeEntry(id: string): ThemeRegistryEntry {
  return {
    kind: "theme",
    id,
    name: "Theme",
    version: "1.0.0",
    summary: "Summary",
    description: "Description",
    author: "Author",
    license: "MIT",
    repository: null,
    categories: [],
    tags: [],
    styles: [],
    icon: null,
    screenshots: [],
    artifact: null,
    theme: {
      id,
      name: "Theme",
      version: 1,
      colors: {
        light: palette("light"),
        dark: palette("dark"),
      },
    },
  };
}

function createPluginEntry(id: string): RegistryPlugin {
  return {
    id,
    name: id,
    version: "1.0.0",
    summary: "Summary",
    description: "Description",
    author: "Author",
    license: "MIT",
    repository: null,
    categories: [],
    tags: [],
    artifact: {
      url: "https://example.com/plugin.wasm",
      sha256: "abc",
      size: 10,
      published_at: "2026-03-04T00:00:00Z",
    },
    capabilities: [],
    icon: null,
    screenshots: [],
    requested_permissions: null,
  };
}

function createTypographyEntry(id: string): TypographyRegistryEntry {
  return {
    kind: "typography",
    id,
    name: "Typography",
    version: "1.0.0",
    summary: "Summary",
    description: "Description",
    author: "Author",
    license: "MIT",
    repository: null,
    categories: [],
    tags: [],
    styles: [],
    icon: null,
    screenshots: [],
    artifact: null,
    typography: {
      id,
      name: "Typography",
      version: 1,
      settings: {
        fontFamily: "serif",
        baseFontSize: 18,
        lineHeight: 1.8,
        contentWidth: "narrow",
      },
    },
  };
}

function createBundle(): BundleRegistryEntry {
  return {
    kind: "bundle",
    id: "bundle.writer",
    name: "Writer",
    version: "1.0.0",
    summary: "Summary",
    description: "Description",
    author: "Author",
    license: "MIT",
    repository: null,
    categories: [],
    tags: [],
    icon: null,
    screenshots: [],
    artifact: null,
    theme_id: "theme.writer",
    typography_id: "typography.editorial-serif",
    typography: {
      fontFamily: "serif",
      baseFontSize: 18,
      lineHeight: 1.8,
      contentWidth: "narrow",
    },
    plugins: [
      { plugin_id: "diaryx.sync", required: true, enable: true },
      { plugin_id: "missing.required", required: true, enable: true },
      { plugin_id: "missing.optional", required: false, enable: true },
    ],
  };
}

describe("bundleApply", () => {
  it("plans and executes in guided best-effort mode", async () => {
    const installedThemes = new Set<string>();
    const installedTypographies = new Set<string>();
    const installedPlugins = new Set<string>();
    const enabledPlugins = new Set<string>();
    let typographyApplied = false;

    const runtime: BundleApplyRuntime = {
      hasTheme(themeId: string): boolean {
        return installedThemes.has(themeId);
      },
      installTheme(theme): void {
        installedThemes.add(theme.id);
      },
      applyTheme(themeId: string): void {
        if (!installedThemes.has(themeId)) {
          throw new Error("Theme not installed");
        }
      },
      hasTypographyPreset(typographyId: string): boolean {
        return installedTypographies.has(typographyId);
      },
      installTypography(typography): void {
        installedTypographies.add(typography.id);
      },
      applyTypographyPreset(typographyId: string): void {
        if (!installedTypographies.has(typographyId)) {
          throw new Error("Typography not installed");
        }
      },
      applyTypographyOverrides(): void {
        typographyApplied = true;
      },
      isPluginInstalled(pluginId: string): boolean {
        return installedPlugins.has(pluginId);
      },
      async installRegistryPlugin(plugin): Promise<void> {
        installedPlugins.add(plugin.id);
      },
      setPluginEnabled(pluginId: string): void {
        enabledPlugins.add(pluginId);
      },
    };

    const bundle = createBundle();
    const plan = planBundleApply(bundle, {
      themes: [createThemeEntry("theme.writer")],
      typographies: [createTypographyEntry("typography.editorial-serif")],
      plugins: [createPluginEntry("diaryx.sync")],
      runtime,
    });

    expect(plan.missingRequiredPlugins).toEqual(["missing.required"]);
    expect(plan.missingOptionalPlugins).toEqual(["missing.optional"]);
    expect(plan.missingTheme).toBe(false);

    const result = await executeBundleApply(plan, runtime);

    expect(installedThemes.has("theme.writer")).toBe(true);
    expect(installedTypographies.has("typography.editorial-serif")).toBe(true);
    expect(typographyApplied).toBe(true);
    expect(installedPlugins.has("diaryx.sync")).toBe(true);
    expect(enabledPlugins.has("diaryx.sync")).toBe(true);
    expect(result.summary.failed).toBeGreaterThan(0);
  });

  it("flags missing theme and reports failure", async () => {
    const runtime: BundleApplyRuntime = {
      hasTheme: () => false,
      installTheme: () => {},
      applyTheme: () => {
        throw new Error("Theme unavailable");
      },
      hasTypographyPreset: () => false,
      installTypography: () => {},
      applyTypographyPreset: () => {},
      applyTypographyOverrides: () => {},
      isPluginInstalled: () => true,
      installRegistryPlugin: async () => {},
      setPluginEnabled: () => {},
    };

    const bundle = createBundle();
    const plan = planBundleApply(bundle, {
      themes: [],
      typographies: [],
      plugins: [],
      runtime,
    });

    expect(plan.missingTheme).toBe(true);

    const result = await executeBundleApply(plan, runtime);
    expect(result.results.some((entry) => entry.message.includes("Theme 'theme.writer'"))).toBe(true);
  });
});
