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
    starter_workspace_id: null,
    spotlight: null,
  };
}

function createStubRuntime(overrides: Partial<BundleApplyRuntime> = {}): BundleApplyRuntime {
  return {
    hasTheme: () => false,
    installTheme: () => {},
    applyTheme: () => {},
    hasTypographyPreset: () => false,
    installTypography: () => {},
    applyTypographyPreset: () => {},
    applyTypographyOverrides: () => {},
    isPluginInstalled: () => false,
    installRegistryPlugin: async () => {},
    setPluginEnabled: () => {},
    ...overrides,
  };
}

describe("bundleApply", () => {
  describe("planBundleApply", () => {
    it("skips theme install when already installed", () => {
      const runtime = createStubRuntime({ hasTheme: () => true });
      const bundle = createBundle();
      const plan = planBundleApply(bundle, {
        themes: [createThemeEntry("theme.writer")],
        typographies: [],
        plugins: [],
        runtime,
      });

      expect(plan.actions.find((a) => a.type === "theme-install")).toBeUndefined();
      expect(plan.actions.find((a) => a.type === "theme-apply")).toBeDefined();
    });

    it("skips typography install when already installed", () => {
      const runtime = createStubRuntime({ hasTypographyPreset: () => true });
      const bundle = createBundle();
      const plan = planBundleApply(bundle, {
        themes: [createThemeEntry("theme.writer")],
        typographies: [createTypographyEntry("typography.editorial-serif")],
        plugins: [],
        runtime,
      });

      expect(plan.actions.find((a) => a.type === "typography-install")).toBeUndefined();
      expect(plan.actions.find((a) => a.type === "typography-preset-apply")).toBeDefined();
    });

    it("skips typography actions when bundle has no typography_id", () => {
      const runtime = createStubRuntime();
      const bundle = createBundle();
      bundle.typography_id = null as any;
      bundle.typography = undefined as any;
      const plan = planBundleApply(bundle, {
        themes: [createThemeEntry("theme.writer")],
        typographies: [],
        plugins: [],
        runtime,
      });

      expect(plan.actions.filter((a) => a.type.startsWith("typography"))).toEqual([]);
    });

    it("flags missing typography preset", () => {
      const runtime = createStubRuntime();
      const bundle = createBundle();
      const plan = planBundleApply(bundle, {
        themes: [createThemeEntry("theme.writer")],
        typographies: [], // no matching typography
        plugins: [],
        runtime,
      });

      expect(plan.missingTypographyPreset).toBe(true);
    });

    it("skips plugin install when already installed", () => {
      const runtime = createStubRuntime({ isPluginInstalled: () => true });
      const bundle = createBundle();
      bundle.plugins = [{ plugin_id: "diaryx.sync", required: true, enable: true }];
      const plan = planBundleApply(bundle, {
        themes: [createThemeEntry("theme.writer")],
        typographies: [],
        plugins: [createPluginEntry("diaryx.sync")],
        runtime,
      });

      expect(plan.actions.find((a) => a.type === "plugin-install")).toBeUndefined();
      expect(plan.actions.find((a) => a.type === "plugin-enable")).toBeDefined();
    });

    it("skips plugin-enable when enable is false", () => {
      const runtime = createStubRuntime();
      const bundle = createBundle();
      bundle.plugins = [{ plugin_id: "diaryx.sync", required: true, enable: false }];
      const plan = planBundleApply(bundle, {
        themes: [createThemeEntry("theme.writer")],
        typographies: [],
        plugins: [createPluginEntry("diaryx.sync")],
        runtime,
      });

      expect(plan.actions.find((a) => a.type === "plugin-enable")).toBeUndefined();
    });

    it("includes typography-override-apply when bundle has typography overrides", () => {
      const runtime = createStubRuntime();
      const bundle = createBundle();
      const plan = planBundleApply(bundle, {
        themes: [createThemeEntry("theme.writer")],
        typographies: [createTypographyEntry("typography.editorial-serif")],
        plugins: [],
        runtime,
      });

      expect(plan.actions.find((a) => a.type === "typography-override-apply")).toBeDefined();
    });
  });

  describe("executeBundleApply", () => {
    it("reports failure for theme-install with missing theme metadata", async () => {
      const runtime = createStubRuntime();
      const plan = {
        bundle: createBundle(),
        actions: [{
          type: "theme-install" as const,
          key: "test",
          required: true,
          label: "Install theme",
          theme: undefined,
        }],
        missingRequiredPlugins: [],
        missingOptionalPlugins: [],
        missingTheme: false,
        missingTypographyPreset: false,
      };

      const result = await executeBundleApply(plan, runtime);
      expect(result.results[0].status).toBe("failed");
      expect(result.results[0].message).toBe("Theme metadata is unavailable");
    });

    it("reports failure for theme-apply with missing themeId", async () => {
      const runtime = createStubRuntime();
      const plan = {
        bundle: createBundle(),
        actions: [{
          type: "theme-apply" as const,
          key: "test",
          required: true,
          label: "Apply theme",
          themeId: undefined,
        }],
        missingRequiredPlugins: [],
        missingOptionalPlugins: [],
        missingTheme: false,
        missingTypographyPreset: false,
      };

      const result = await executeBundleApply(plan, runtime);
      expect(result.results[0].status).toBe("failed");
      expect(result.results[0].message).toBe("Theme ID is unavailable");
    });

    it("reports failure for typography-install with missing entry", async () => {
      const runtime = createStubRuntime();
      const plan = {
        bundle: createBundle(),
        actions: [{
          type: "typography-install" as const,
          key: "test",
          required: false,
          label: "Install typography",
          typographyEntry: undefined,
        }],
        missingRequiredPlugins: [],
        missingOptionalPlugins: [],
        missingTheme: false,
        missingTypographyPreset: false,
      };

      const result = await executeBundleApply(plan, runtime);
      expect(result.results[0].status).toBe("failed");
      expect(result.results[0].message).toBe("Typography metadata is unavailable");
    });

    it("reports failure for typography-preset-apply with missing id", async () => {
      const runtime = createStubRuntime();
      const plan = {
        bundle: createBundle(),
        actions: [{
          type: "typography-preset-apply" as const,
          key: "test",
          required: false,
          label: "Apply typography",
          typographyId: undefined,
        }],
        missingRequiredPlugins: [],
        missingOptionalPlugins: [],
        missingTheme: false,
        missingTypographyPreset: false,
      };

      const result = await executeBundleApply(plan, runtime);
      expect(result.results[0].status).toBe("failed");
      expect(result.results[0].message).toBe("Typography preset ID is unavailable");
    });

    it("reports failure for typography-override-apply with missing overrides", async () => {
      const runtime = createStubRuntime();
      const plan = {
        bundle: createBundle(),
        actions: [{
          type: "typography-override-apply" as const,
          key: "test",
          required: false,
          label: "Apply overrides",
          typography: undefined,
        }],
        missingRequiredPlugins: [],
        missingOptionalPlugins: [],
        missingTheme: false,
        missingTypographyPreset: false,
      };

      const result = await executeBundleApply(plan, runtime);
      expect(result.results[0].status).toBe("failed");
      expect(result.results[0].message).toBe("Typography overrides are unavailable");
    });

    it("reports failure for plugin-install with missing plugin", async () => {
      const runtime = createStubRuntime();
      const plan = {
        bundle: createBundle(),
        actions: [{
          type: "plugin-install" as const,
          key: "test",
          required: true,
          label: "Install plugin",
          plugin: undefined,
        }],
        missingRequiredPlugins: [],
        missingOptionalPlugins: [],
        missingTheme: false,
        missingTypographyPreset: false,
      };

      const result = await executeBundleApply(plan, runtime);
      expect(result.results[0].status).toBe("failed");
      expect(result.results[0].message).toBe("Plugin metadata is unavailable");
    });

    it("reports failure for plugin-enable with missing pluginId", async () => {
      const runtime = createStubRuntime();
      const plan = {
        bundle: createBundle(),
        actions: [{
          type: "plugin-enable" as const,
          key: "test",
          required: true,
          label: "Enable plugin",
          pluginId: undefined,
        }],
        missingRequiredPlugins: [],
        missingOptionalPlugins: [],
        missingTheme: false,
        missingTypographyPreset: false,
      };

      const result = await executeBundleApply(plan, runtime);
      expect(result.results[0].status).toBe("failed");
      expect(result.results[0].message).toBe("Plugin ID is unavailable");
    });

    it("reports failure for plugin-enable when plugin is not installed", async () => {
      const runtime = createStubRuntime({ isPluginInstalled: () => false });
      const plan = {
        bundle: createBundle(),
        actions: [{
          type: "plugin-enable" as const,
          key: "test",
          required: true,
          label: "Enable plugin",
          pluginId: "some.plugin",
          pluginEnable: true,
        }],
        missingRequiredPlugins: [],
        missingOptionalPlugins: [],
        missingTheme: false,
        missingTypographyPreset: false,
      };

      const result = await executeBundleApply(plan, runtime);
      expect(result.results[0].status).toBe("failed");
      expect(result.results[0].message).toBe("Plugin is not installed");
    });

    it("appends missing typography failure to results", async () => {
      const runtime = createStubRuntime();
      const bundle = createBundle();
      const plan = {
        bundle,
        actions: [],
        missingRequiredPlugins: [],
        missingOptionalPlugins: [],
        missingTheme: false,
        missingTypographyPreset: true,
      };

      const result = await executeBundleApply(plan, runtime);
      expect(result.results.some((r) =>
        r.message.includes("Typography preset 'typography.editorial-serif'"),
      )).toBe(true);
    });

    it("appends missing required and optional plugin failures", async () => {
      const runtime = createStubRuntime();
      const plan = {
        bundle: createBundle(),
        actions: [],
        missingRequiredPlugins: ["required.plugin"],
        missingOptionalPlugins: ["optional.plugin"],
        missingTheme: false,
        missingTypographyPreset: false,
      };

      const result = await executeBundleApply(plan, runtime);
      expect(result.results.find((r) => r.message.includes("Required plugin 'required.plugin'"))).toBeDefined();
      expect(result.results.find((r) => r.message.includes("Optional plugin 'optional.plugin'"))).toBeDefined();
    });

    it("computes correct summary counts", async () => {
      const runtime = createStubRuntime({
        hasTheme: () => true,
        isPluginInstalled: () => true,
      });
      const bundle = createBundle();
      bundle.plugins = [{ plugin_id: "diaryx.sync", required: true, enable: true }];
      bundle.typography = undefined as any;
      const plan = planBundleApply(bundle, {
        themes: [createThemeEntry("theme.writer")],
        typographies: [createTypographyEntry("typography.editorial-serif")],
        plugins: [createPluginEntry("diaryx.sync")],
        runtime,
      });

      const result = await executeBundleApply(plan, runtime);
      expect(result.summary.total).toBe(result.summary.success + result.summary.failed);
    });

    it("stringifies non-Error throws", async () => {
      const runtime = createStubRuntime({
        applyTheme: () => { throw "raw string error"; },
      });
      const plan = {
        bundle: createBundle(),
        actions: [{
          type: "theme-apply" as const,
          key: "test",
          required: true,
          label: "Apply theme",
          themeId: "theme.writer",
        }],
        missingRequiredPlugins: [],
        missingOptionalPlugins: [],
        missingTheme: false,
        missingTypographyPreset: false,
      };

      const result = await executeBundleApply(plan, runtime);
      expect(result.results[0].status).toBe("failed");
      expect(result.results[0].message).toBe("raw string error");
    });
  });

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
