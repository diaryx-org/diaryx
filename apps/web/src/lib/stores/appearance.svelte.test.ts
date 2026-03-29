import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  BUILTIN_PRESETS,
  BUILTIN_PRESET_LIST,
  BUILTIN_TYPOGRAPHY_PRESETS,
  BUILTIN_TYPOGRAPHY_PRESET_LIST,
} from "./appearance.presets";
import { createAppearanceStore } from "./appearance.svelte";
import type {
  ThemeDefinition,
  TypographyDefinition,
  UserAppearance,
} from "./appearance.types";

// ---------------------------------------------------------------------------
// Mock workspace persistence (async I/O not needed in unit tests)
// ---------------------------------------------------------------------------

vi.mock("$lib/workspace/workspaceAssetStorage", () => ({
  readWorkspaceText: vi.fn().mockResolvedValue(null),
  writeWorkspaceText: vi.fn().mockResolvedValue(undefined),
  getThemeSettingsPath: vi.fn().mockReturnValue(".diaryx/themes/settings.json"),
  getThemeLibraryPath: vi.fn().mockReturnValue(".diaryx/themes/library.json"),
  getTypographySettingsPath: vi.fn().mockReturnValue(".diaryx/typographies/settings.json"),
  getTypographyLibraryPath: vi.fn().mockReturnValue(".diaryx/typographies/library.json"),
  getThemeModePath: vi.fn().mockReturnValue(".diaryx/themes/mode.json"),
}));

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function cloneCustomTheme(id: string, name: string): ThemeDefinition {
  const base = BUILTIN_PRESETS.default;
  return {
    ...base,
    id,
    name,
    colors: {
      light: { ...base.colors.light },
      dark: { ...base.colors.dark },
    },
  };
}

function cloneCustomTypography(id: string, name: string): TypographyDefinition {
  const base = BUILTIN_TYPOGRAPHY_PRESETS.default;
  return {
    ...base,
    id,
    name,
    settings: { ...base.settings },
  };
}

beforeEach(() => {
  localStorage.clear();

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
// Theme library
// ===========================================================================

describe("appearanceStore theme library", () => {
  it("installs, applies, and uninstalls custom themes with fallback", () => {
    const store = createAppearanceStore();
    const customTheme = cloneCustomTheme("custom.sunset", "Sunset");

    expect(store.installTheme(customTheme, { source: "local" })).toBe(true);
    expect(store.listThemes().some((entry) => entry.theme.id === customTheme.id)).toBe(true);

    expect(store.applyTheme(customTheme.id)).toBe(true);
    expect(store.presetId).toBe(customTheme.id);

    expect(store.uninstallTheme(customTheme.id)).toBe(true);
    expect(store.presetId).toBe("default");
    expect(store.listThemes().some((entry) => entry.theme.id === customTheme.id)).toBe(false);
  });

  it("preserves accent override when applying themes", () => {
    const store = createAppearanceStore();
    const customTheme = cloneCustomTheme("custom.ocean", "Ocean");

    store.setAccentHue(210);
    expect(store.installTheme(customTheme, { source: "local" })).toBe(true);
    expect(store.applyTheme(customTheme.id)).toBe(true);

    expect(store.accentHue).toBe(210);
  });

  it("routes importTheme through install + apply", () => {
    const store = createAppearanceStore();
    const customTheme = cloneCustomTheme("custom.imported", "Imported");

    expect(
      store.importTheme({
        $schema: "https://diaryx.com/schemas/theme/v1",
        theme: customTheme,
      }),
    ).toBe(true);

    expect(store.presetId).toBe(customTheme.id);
    expect(store.listThemes().some((entry) => entry.theme.id === customTheme.id)).toBe(true);
  });

  it("rejects installing an invalid theme", () => {
    const store = createAppearanceStore();
    const invalidTheme = { id: "", name: "", version: 1 } as unknown as ThemeDefinition;
    expect(store.installTheme(invalidTheme)).toBe(false);
  });

  it("returns true when installing a builtin theme id (noop)", () => {
    const store = createAppearanceStore();
    expect(store.installTheme(BUILTIN_PRESETS.default)).toBe(true);
  });

  it("refuses to uninstall a builtin theme", () => {
    const store = createAppearanceStore();
    expect(store.uninstallTheme("default")).toBe(false);
  });

  it("refuses to uninstall a theme that is not installed", () => {
    const store = createAppearanceStore();
    expect(store.uninstallTheme("nonexistent.theme")).toBe(false);
  });

  it("returns false when applying a nonexistent theme", () => {
    const store = createAppearanceStore();
    expect(store.applyTheme("nonexistent.theme")).toBe(false);
  });

  it("importTheme rejects invalid data", () => {
    const store = createAppearanceStore();
    expect(store.importTheme(null)).toBe(false);
    expect(store.importTheme(42)).toBe(false);
    expect(store.importTheme({ theme: { id: "", name: "" } })).toBe(false);
  });

  it("importTheme accepts a bare theme definition (no wrapper)", () => {
    const store = createAppearanceStore();
    const customTheme = cloneCustomTheme("custom.bare", "Bare");
    expect(store.importTheme(customTheme)).toBe(true);
    expect(store.presetId).toBe("custom.bare");
  });

  it("listThemes includes builtins and installed", () => {
    const store = createAppearanceStore();
    const custom = cloneCustomTheme("custom.list", "List Theme");
    store.installTheme(custom, { source: "local" });

    const list = store.listThemes();
    const builtinCount = BUILTIN_PRESET_LIST.length;
    expect(list.length).toBe(builtinCount + 1);
    expect(list.filter((e) => e.builtin).length).toBe(builtinCount);
    expect(list.filter((e) => !e.builtin).length).toBe(1);
  });

  it("isBuiltinTheme identifies builtins correctly", () => {
    const store = createAppearanceStore();
    expect(store.isBuiltinTheme("default")).toBe(true);
    expect(store.isBuiltinTheme("sepia")).toBe(true);
    expect(store.isBuiltinTheme("custom.something")).toBe(false);
  });

  it("persists installed themes across store instances", () => {
    const first = createAppearanceStore();
    const custom = cloneCustomTheme("custom.persist", "Persist");
    first.installTheme(custom, { source: "local" });
    first.applyTheme(custom.id);

    const second = createAppearanceStore();
    expect(second.presetId).toBe(custom.id);
    expect(second.listThemes().some((e) => e.theme.id === custom.id)).toBe(true);
  });

  it("uninstallTheme of a non-active theme does not reset presetId", () => {
    const store = createAppearanceStore();
    const theme1 = cloneCustomTheme("custom.a", "A");
    const theme2 = cloneCustomTheme("custom.b", "B");
    store.installTheme(theme1, { source: "local" });
    store.installTheme(theme2, { source: "local" });
    store.applyTheme(theme1.id);

    store.uninstallTheme(theme2.id);
    expect(store.presetId).toBe(theme1.id);
  });
});

// ===========================================================================
// Typography library
// ===========================================================================

describe("appearanceStore typography library", () => {
  it("installs, applies, and uninstalls custom typography with fallback", () => {
    const store = createAppearanceStore();
    const customTypography = cloneCustomTypography(
      "custom.editorial",
      "Editorial",
    );

    customTypography.settings = {
      fontFamily: "serif",
      baseFontSize: 19,
      lineHeight: 1.85,
      contentWidth: "narrow",
    };

    expect(store.installTypography(customTypography, { source: "local" })).toBe(true);
    expect(
      store
        .listTypographies()
        .some((entry) => entry.typography.id === customTypography.id),
    ).toBe(true);

    expect(store.applyTypographyPreset(customTypography.id)).toBe(true);
    expect(store.typographyPresetId).toBe(customTypography.id);
    expect(store.typography.baseFontSize).toBe(19);

    store.setBaseFontSize(20);
    expect(store.typographyOverrides.baseFontSize).toBe(20);

    expect(store.uninstallTypography(customTypography.id)).toBe(true);
    expect(store.typographyPresetId).toBe("default");
    expect(store.typographyOverrides).toEqual({});
  });

  it("persists installed typography presets across store instances", () => {
    const first = createAppearanceStore();
    const customTypography = cloneCustomTypography("custom.mono", "Mono");

    customTypography.settings = {
      fontFamily: "mono",
      baseFontSize: 15,
      lineHeight: 1.55,
      contentWidth: "full",
    };

    expect(first.installTypography(customTypography, { source: "local" })).toBe(true);
    expect(first.applyTypographyPreset(customTypography.id)).toBe(true);

    const second = createAppearanceStore();
    expect(
      second
        .listTypographies()
        .some((entry) => entry.typography.id === customTypography.id),
    ).toBe(true);
    expect(second.typographyPresetId).toBe(customTypography.id);
  });

  it("keeps per-field overrides adjustable on top of preset settings", () => {
    const store = createAppearanceStore();

    expect(store.applyTypographyPreset("editorial-serif")).toBe(true);
    expect(store.typography.fontFamily).toBe("serif");

    store.setBaseFontSize(17);
    expect(store.typography.baseFontSize).toBe(17);
    expect(store.typographyOverrides.baseFontSize).toBe(17);

    // Setting a field back to preset value removes the override key.
    store.setBaseFontSize(18);
    expect(store.typography.baseFontSize).toBe(18);
    expect(store.typographyOverrides.baseFontSize).toBeUndefined();
  });

  it("rejects installing invalid typography", () => {
    const store = createAppearanceStore();
    const invalid = { id: "", name: "", version: 1, settings: {} } as unknown as TypographyDefinition;
    expect(store.installTypography(invalid)).toBe(false);
  });

  it("returns true when installing a builtin typography id (noop)", () => {
    const store = createAppearanceStore();
    expect(store.installTypography(BUILTIN_TYPOGRAPHY_PRESETS.default)).toBe(true);
  });

  it("refuses to uninstall a builtin typography", () => {
    const store = createAppearanceStore();
    expect(store.uninstallTypography("default")).toBe(false);
  });

  it("refuses to uninstall a typography that is not installed", () => {
    const store = createAppearanceStore();
    expect(store.uninstallTypography("nonexistent")).toBe(false);
  });

  it("returns false when applying a nonexistent typography", () => {
    const store = createAppearanceStore();
    expect(store.applyTypographyPreset("nonexistent")).toBe(false);
  });

  it("isBuiltinTypography identifies builtins correctly", () => {
    const store = createAppearanceStore();
    expect(store.isBuiltinTypography("default")).toBe(true);
    expect(store.isBuiltinTypography("editorial-serif")).toBe(true);
    expect(store.isBuiltinTypography("custom.something")).toBe(false);
  });

  it("listTypographies includes builtins and installed", () => {
    const store = createAppearanceStore();
    const custom = cloneCustomTypography("custom.list-typo", "List Typo");
    custom.settings = { fontFamily: "mono", baseFontSize: 14, lineHeight: 1.4, contentWidth: "wide" };
    store.installTypography(custom, { source: "local" });

    const list = store.listTypographies();
    const builtinCount = BUILTIN_TYPOGRAPHY_PRESET_LIST.length;
    expect(list.length).toBe(builtinCount + 1);
    expect(list.filter((e) => e.builtin).length).toBe(builtinCount);
  });

  it("importTypography installs and applies", () => {
    const store = createAppearanceStore();
    const custom = cloneCustomTypography("custom.imported-typo", "Imported Typo");
    custom.settings = { fontFamily: "serif", baseFontSize: 20, lineHeight: 1.8, contentWidth: "narrow" };

    expect(
      store.importTypography({
        $schema: "https://diaryx.com/schemas/typography/v1",
        typography: custom,
      }),
    ).toBe(true);
    expect(store.typographyPresetId).toBe(custom.id);
  });

  it("importTypography rejects invalid data", () => {
    const store = createAppearanceStore();
    expect(store.importTypography(null)).toBe(false);
    expect(store.importTypography(42)).toBe(false);
    expect(store.importTypography({ typography: { id: "", name: "" } })).toBe(false);
  });

  it("uninstallTypography of a non-active typography does not change typographyPresetId", () => {
    const store = createAppearanceStore();
    const typo1 = cloneCustomTypography("custom.x", "X");
    typo1.settings = { fontFamily: "mono", baseFontSize: 14, lineHeight: 1.4, contentWidth: "wide" };
    const typo2 = cloneCustomTypography("custom.y", "Y");
    typo2.settings = { fontFamily: "serif", baseFontSize: 18, lineHeight: 1.7, contentWidth: "narrow" };

    store.installTypography(typo1, { source: "local" });
    store.installTypography(typo2, { source: "local" });
    store.applyTypographyPreset(typo1.id);

    store.uninstallTypography(typo2.id);
    expect(store.typographyPresetId).toBe(typo1.id);
  });

  it("applyTypographyPreset with preserveOverrides keeps existing overrides", () => {
    const store = createAppearanceStore();
    store.setBaseFontSize(20);
    expect(store.typographyOverrides.baseFontSize).toBe(20);

    store.applyTypographyPreset("editorial-serif", { preserveOverrides: true });
    expect(store.typographyPresetId).toBe("editorial-serif");
    // The 20px override differs from editorial-serif's 18px, so it is kept
    expect(store.typographyOverrides.baseFontSize).toBe(20);
  });
});

// ===========================================================================
// Appearance getters and setters
// ===========================================================================

describe("appearanceStore getters and setters", () => {
  it("defaults to sensible initial values", () => {
    const store = createAppearanceStore();
    expect(store.presetId).toBe("default");
    expect(store.typographyPresetId).toBe("default");
    expect(store.accentHue).toBeNull();
    expect(store.highContrastEditor).toBe(false);
    expect(store.typographyOverrides).toEqual({});
  });

  it("setPreset changes the active preset", () => {
    const store = createAppearanceStore();
    store.setPreset("sepia");
    expect(store.presetId).toBe("sepia");
  });

  it("setPreset ignores unknown preset ids", () => {
    const store = createAppearanceStore();
    store.setPreset("nonexistent");
    expect(store.presetId).toBe("default");
  });

  it("setAccentHue sets and clears accent", () => {
    const store = createAppearanceStore();
    store.setAccentHue(180);
    expect(store.accentHue).toBe(180);

    store.setAccentHue(null);
    expect(store.accentHue).toBeNull();
  });

  it("setHighContrastEditor toggles the flag", () => {
    const store = createAppearanceStore();
    store.setHighContrastEditor(true);
    expect(store.highContrastEditor).toBe(true);

    store.setHighContrastEditor(false);
    expect(store.highContrastEditor).toBe(false);
  });

  it("setFontFamily creates a typography override", () => {
    const store = createAppearanceStore();
    store.setFontFamily("serif");
    expect(store.typographyOverrides.fontFamily).toBe("serif");
    expect(store.typography.fontFamily).toBe("serif");
  });

  it("setBaseFontSize creates a typography override", () => {
    const store = createAppearanceStore();
    store.setBaseFontSize(20);
    expect(store.typographyOverrides.baseFontSize).toBe(20);
    expect(store.typography.baseFontSize).toBe(20);
  });

  it("setLineHeight creates a typography override", () => {
    const store = createAppearanceStore();
    store.setLineHeight(2.0);
    expect(store.typographyOverrides.lineHeight).toBe(2.0);
    expect(store.typography.lineHeight).toBe(2.0);
  });

  it("setContentWidth creates a typography override", () => {
    const store = createAppearanceStore();
    store.setContentWidth("wide");
    expect(store.layout.contentWidth).toBe("wide");
  });

  it("clearTypographyOverrides removes all overrides", () => {
    const store = createAppearanceStore();
    store.setBaseFontSize(20);
    store.setFontFamily("mono");
    expect(Object.keys(store.typographyOverrides).length).toBeGreaterThan(0);

    store.clearTypographyOverrides();
    expect(store.typographyOverrides).toEqual({});
  });

  it("setTypographyPreset changes active typography preset", () => {
    const store = createAppearanceStore();
    store.setTypographyPreset("editorial-serif");
    expect(store.typographyPresetId).toBe("editorial-serif");
  });

  it("typography getter resolves effective settings from preset + overrides", () => {
    const store = createAppearanceStore();
    expect(store.typography.fontFamily).toBe("inter");
    expect(store.typography.baseFontSize).toBe(16);
    expect(store.typography.lineHeight).toBe(1.6);

    store.setBaseFontSize(18);
    expect(store.typography.baseFontSize).toBe(18);
    expect(store.typography.fontFamily).toBe("inter");
  });

  it("activePreset returns the current theme definition", () => {
    const store = createAppearanceStore();
    expect(store.activePreset.id).toBe("default");

    store.setPreset("nord");
    expect(store.activePreset.id).toBe("nord");
  });

  it("activeTypographyPreset returns the current typography definition", () => {
    const store = createAppearanceStore();
    expect(store.activeTypographyPreset.id).toBe("default");

    store.setTypographyPreset("compact-system");
    expect(store.activeTypographyPreset.id).toBe("compact-system");
  });

  it("appearance getter returns the full appearance object", () => {
    const store = createAppearanceStore();
    const app = store.appearance;
    expect(app.presetId).toBe("default");
    expect(app.accentHue).toBeNull();
    expect(app.typographyPresetId).toBe("default");
    expect(app.typographyOverrides).toEqual({});
    expect(app.highContrastEditor).toBe(false);
  });

  it("layout getter returns contentWidth from resolved settings", () => {
    const store = createAppearanceStore();
    expect(store.layout.contentWidth).toBe("medium");
    store.setContentWidth("full");
    expect(store.layout.contentWidth).toBe("full");
  });
});

// ===========================================================================
// Export / import
// ===========================================================================

describe("appearanceStore export / import", () => {
  it("exportTheme returns current theme with schema", () => {
    const store = createAppearanceStore();
    store.setPreset("sepia");
    const exported = store.exportTheme();
    expect(exported.$schema).toBe("https://diaryx.com/schemas/theme/v1");
    expect(exported.theme.id).toBe("sepia");
  });

  it("exportTypography returns current typography with schema", () => {
    const store = createAppearanceStore();
    store.setTypographyPreset("compact-system");
    const exported = store.exportTypography();
    expect(exported.$schema).toBe("https://diaryx.com/schemas/typography/v1");
    expect(exported.typography.id).toBe("compact-system");
  });

  it("round-trips theme export and import", () => {
    const store = createAppearanceStore();
    const custom = cloneCustomTheme("custom.roundtrip", "Roundtrip");
    store.installTheme(custom, { source: "local" });
    store.applyTheme(custom.id);

    const exported = store.exportTheme();

    const store2 = createAppearanceStore();
    expect(store2.importTheme(exported)).toBe(true);
    expect(store2.presetId).toBe("custom.roundtrip");
  });

  it("round-trips typography export and import", () => {
    const store = createAppearanceStore();
    const custom = cloneCustomTypography("custom.roundtrip-typo", "RT Typo");
    custom.settings = { fontFamily: "mono", baseFontSize: 15, lineHeight: 1.5, contentWidth: "full" };
    store.installTypography(custom, { source: "local" });
    store.applyTypographyPreset(custom.id);

    const exported = store.exportTypography();

    const store2 = createAppearanceStore();
    expect(store2.importTypography(exported)).toBe(true);
    expect(store2.typographyPresetId).toBe("custom.roundtrip-typo");
  });

  it("exportTheme returns a deep clone", () => {
    const store = createAppearanceStore();
    const exported1 = store.exportTheme();
    const exported2 = store.exportTheme();
    expect(exported1).toEqual(exported2);
    expect(exported1.theme.colors.light).not.toBe(exported2.theme.colors.light);
  });
});

// ===========================================================================
// Reset
// ===========================================================================

describe("appearanceStore reset", () => {
  it("reset restores defaults", () => {
    const store = createAppearanceStore();
    store.setPreset("sepia");
    store.setAccentHue(200);
    store.setHighContrastEditor(true);
    store.setFontFamily("mono");
    store.setTypographyPreset("editorial-serif");

    store.reset();
    expect(store.presetId).toBe("default");
    expect(store.accentHue).toBeNull();
    expect(store.highContrastEditor).toBe(false);
    expect(store.typographyOverrides).toEqual({});
  });

  it("reset clears high-contrast-editor class from document", () => {
    const store = createAppearanceStore();
    store.setHighContrastEditor(true);
    expect(document.documentElement.classList.contains("high-contrast-editor")).toBe(true);

    store.reset();
    expect(document.documentElement.classList.contains("high-contrast-editor")).toBe(false);
  });
});

// ===========================================================================
// Persistence / loadAppearance
// ===========================================================================

describe("appearanceStore persistence", () => {
  it("loads appearance from localStorage on creation", () => {
    const saved: UserAppearance = {
      presetId: "nord",
      accentHue: 120,
      typographyPresetId: "compact-system",
      typographyOverrides: { baseFontSize: 14 },
      highContrastEditor: true,
    };
    localStorage.setItem("diaryx-appearance", JSON.stringify(saved));

    const store = createAppearanceStore();
    expect(store.presetId).toBe("nord");
    expect(store.accentHue).toBe(120);
    expect(store.typographyPresetId).toBe("compact-system");
    expect(store.highContrastEditor).toBe(true);
  });

  it("falls back to defaults on corrupt localStorage data", () => {
    localStorage.setItem("diaryx-appearance", "not-json!!!");

    const store = createAppearanceStore();
    expect(store.presetId).toBe("default");
    expect(store.accentHue).toBeNull();
  });

  it("falls back to defaults on non-object parsed data", () => {
    localStorage.setItem("diaryx-appearance", JSON.stringify("a string"));

    const store = createAppearanceStore();
    expect(store.presetId).toBe("default");
  });

  it("normalizes unknown presetId to default", () => {
    localStorage.setItem(
      "diaryx-appearance",
      JSON.stringify({
        presetId: "nonexistent",
        accentHue: null,
        typographyPresetId: "default",
        typographyOverrides: {},
        highContrastEditor: false,
      }),
    );

    const store = createAppearanceStore();
    expect(store.presetId).toBe("default");
  });

  it("normalizes unknown typographyPresetId to default", () => {
    localStorage.setItem(
      "diaryx-appearance",
      JSON.stringify({
        presetId: "default",
        accentHue: null,
        typographyPresetId: "nonexistent-typo",
        typographyOverrides: {},
        highContrastEditor: false,
      }),
    );

    const store = createAppearanceStore();
    expect(store.typographyPresetId).toBe("default");
  });

  it("migrates legacy readable line length setting", () => {
    localStorage.setItem("diaryx-readable-line-length", "false");

    const store = createAppearanceStore();
    expect(store.layout.contentWidth).toBe("full");
  });

  it("saves appearance to localStorage on changes", () => {
    const store = createAppearanceStore();
    store.setAccentHue(90);

    const raw = localStorage.getItem("diaryx-appearance");
    expect(raw).toBeTruthy();
    const parsed = JSON.parse(raw!);
    expect(parsed.accentHue).toBe(90);
  });

  it("loads legacy typography fields from old format", () => {
    localStorage.setItem(
      "diaryx-appearance",
      JSON.stringify({
        presetId: "default",
        accentHue: null,
        typographyPresetId: "default",
        typography: { fontFamily: "serif", baseFontSize: 18, lineHeight: 1.8 },
        layout: { contentWidth: "narrow" },
        highContrastEditor: false,
      }),
    );

    const store = createAppearanceStore();
    expect(store.typography.fontFamily).toBe("serif");
    expect(store.typography.baseFontSize).toBe(18);
    expect(store.typography.lineHeight).toBe(1.8);
    expect(store.layout.contentWidth).toBe("narrow");
  });

  it("handles null accentHue in localStorage correctly", () => {
    localStorage.setItem(
      "diaryx-appearance",
      JSON.stringify({
        presetId: "sepia",
        accentHue: null,
        typographyPresetId: "default",
        typographyOverrides: {},
        highContrastEditor: false,
      }),
    );

    const store = createAppearanceStore();
    expect(store.accentHue).toBeNull();
  });

  it("persists theme library to localStorage", () => {
    const store = createAppearanceStore();
    const custom = cloneCustomTheme("custom.stored", "Stored");
    store.installTheme(custom, { source: "local" });

    const raw = localStorage.getItem("diaryx-theme-library-v1");
    expect(raw).toBeTruthy();
    const parsed = JSON.parse(raw!);
    expect(Array.isArray(parsed)).toBe(true);
    expect(parsed.some((e: any) => e.theme?.id === "custom.stored")).toBe(true);
  });

  it("persists typography library to localStorage", () => {
    const store = createAppearanceStore();
    const custom = cloneCustomTypography("custom.stored-typo", "Stored Typo");
    custom.settings = { fontFamily: "serif", baseFontSize: 20, lineHeight: 1.8, contentWidth: "narrow" };
    store.installTypography(custom, { source: "local" });

    const raw = localStorage.getItem("diaryx-typography-library-v1");
    expect(raw).toBeTruthy();
    const parsed = JSON.parse(raw!);
    expect(Array.isArray(parsed)).toBe(true);
    expect(parsed.some((e: any) => e.typography?.id === "custom.stored-typo")).toBe(true);
  });
});
