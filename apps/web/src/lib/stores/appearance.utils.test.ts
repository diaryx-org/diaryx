import { beforeEach, describe, expect, it } from "vitest";

import {
  parseOklch,
  toOklchString,
  shiftHue,
  applyAccentHue,
  applyCssVars,
  clearCssVars,
  cacheVarsForFouc,
  clearVarsCache,
  resolveEffectivePalette,
  DEFAULT_APPEARANCE,
} from "./appearance.utils";
import { BUILTIN_PRESETS } from "./appearance.presets";
import type { TypographySettings } from "./appearance.types";

beforeEach(() => {
  localStorage.clear();
  // Clear any inline styles from previous tests
  document.documentElement.style.cssText = "";
  delete (globalThis as any).__diaryx_preview;
});

// ===========================================================================
// parseOklch
// ===========================================================================

describe("parseOklch", () => {
  it("parses a standard oklch string", () => {
    const result = parseOklch("oklch(0.5 0.1 200)");
    expect(result).toEqual({ L: 0.5, C: 0.1, H: 200 });
  });

  it("parses oklch with decimal values", () => {
    const result = parseOklch("oklch(0.929 0.013 255.508)");
    expect(result).toEqual({ L: 0.929, C: 0.013, H: 255.508 });
  });

  it("parses oklch with alpha", () => {
    const result = parseOklch("oklch(1 0 0 / 10%)");
    expect(result).toEqual({ L: 1, C: 0, H: 0 });
  });

  it("parses oklch with decimal alpha", () => {
    const result = parseOklch("oklch(0.5 0.1 200 / 0.5)");
    expect(result).toEqual({ L: 0.5, C: 0.1, H: 200 });
  });

  it("returns null for non-oklch strings", () => {
    expect(parseOklch("rgb(255, 0, 0)")).toBeNull();
    expect(parseOklch("#ff0000")).toBeNull();
    expect(parseOklch("red")).toBeNull();
    expect(parseOklch("")).toBeNull();
  });
});

// ===========================================================================
// toOklchString
// ===========================================================================

describe("toOklchString", () => {
  it("rebuilds an oklch string from components", () => {
    expect(toOklchString({ L: 0.5, C: 0.1, H: 200 })).toBe("oklch(0.5 0.1 200)");
  });

  it("handles zero values", () => {
    expect(toOklchString({ L: 0, C: 0, H: 0 })).toBe("oklch(0 0 0)");
  });

  it("handles decimal precision", () => {
    expect(toOklchString({ L: 0.929, C: 0.013, H: 255.508 })).toBe(
      "oklch(0.929 0.013 255.508)",
    );
  });
});

// ===========================================================================
// shiftHue
// ===========================================================================

describe("shiftHue", () => {
  it("replaces the hue in a standard oklch string", () => {
    const result = shiftHue("oklch(0.5 0.1 200)", 120);
    expect(result).toBe("oklch(0.5 0.1 120)");
  });

  it("replaces the hue in an oklch string with alpha", () => {
    const result = shiftHue("oklch(0.5 0.1 200 / 10%)", 90);
    // The alpha capture group includes the leading slash and spacing as-is
    expect(result).toBe("oklch(0.5 0.1 90/ 10%)");
  });

  it("returns the original string for non-oklch values", () => {
    expect(shiftHue("rgb(255,0,0)", 120)).toBe("rgb(255,0,0)");
  });

  it("preserves L and C components", () => {
    const result = shiftHue("oklch(0.929 0.013 255.508)", 0);
    expect(result).toBe("oklch(0.929 0.013 0)");
  });
});

// ===========================================================================
// applyAccentHue
// ===========================================================================

describe("applyAccentHue", () => {
  it("shifts hue of accent keys while preserving destructive", () => {
    const palette = BUILTIN_PRESETS.default.colors.light;
    const shifted = applyAccentHue(palette, 120);

    // Accent keys should be shifted
    expect(shifted.primary).toContain("120");
    expect(shifted.ring).toContain("120");
    expect(shifted.accent).toContain("120");
    expect(shifted["sidebar-primary"]).toContain("120");
    expect(shifted["sidebar-ring"]).toContain("120");

    // Destructive should NOT be shifted
    expect(shifted.destructive).toBe(palette.destructive);

    // Non-accent keys should not be shifted
    expect(shifted.background).toBe(palette.background);
    expect(shifted.foreground).toBe(palette.foreground);
  });

  it("returns a new object (does not mutate input)", () => {
    const palette = BUILTIN_PRESETS.default.colors.light;
    const shifted = applyAccentHue(palette, 120);
    expect(shifted).not.toBe(palette);
    // Original unchanged
    expect(palette.primary).not.toContain("120");
  });
});

// ===========================================================================
// applyCssVars
// ===========================================================================

describe("applyCssVars", () => {
  const defaultPalette = BUILTIN_PRESETS.default.colors.light;
  const defaultTypography: TypographySettings = {
    fontFamily: "inter",
    baseFontSize: 16,
    lineHeight: 1.6,
    contentWidth: "medium",
  };

  it("sets CSS custom properties on documentElement", () => {
    applyCssVars(defaultPalette, defaultTypography);
    const style = document.documentElement.style;

    expect(style.getPropertyValue("--background")).toBe(defaultPalette.background);
    expect(style.getPropertyValue("--foreground")).toBe(defaultPalette.foreground);
    expect(style.getPropertyValue("--primary")).toBe(defaultPalette.primary);
    expect(style.getPropertyValue("--destructive")).toBe(defaultPalette.destructive);
  });

  it("sets typography CSS variables", () => {
    applyCssVars(defaultPalette, defaultTypography);
    const style = document.documentElement.style;

    expect(style.getPropertyValue("--editor-font-size")).toBe("16px");
    expect(style.getPropertyValue("--editor-line-height")).toBe("1.6");
    expect(style.getPropertyValue("--editor-content-max-width")).toBe("65ch");
  });

  it("returns a record of all applied vars", () => {
    const vars = applyCssVars(defaultPalette, defaultTypography);

    expect(vars["--background"]).toBe(defaultPalette.background);
    expect(vars["--editor-font-size"]).toBe("16px");
    expect(vars["--editor-line-height"]).toBe("1.6");
    expect(vars["--editor-content-max-width"]).toBe("65ch");
    expect(vars["--editor-font-family"]).toContain("Inter");
  });

  it("maps font family correctly", () => {
    const monoTypography: TypographySettings = {
      ...defaultTypography,
      fontFamily: "mono",
    };
    const vars = applyCssVars(defaultPalette, monoTypography);
    expect(vars["--editor-font-family"]).toContain("SF Mono");
  });

  it("maps content width correctly", () => {
    const narrowTypo: TypographySettings = { ...defaultTypography, contentWidth: "narrow" };
    const vars = applyCssVars(defaultPalette, narrowTypo);
    expect(vars["--editor-content-max-width"]).toBe("55ch");

    const wideTypo: TypographySettings = { ...defaultTypography, contentWidth: "wide" };
    const vars2 = applyCssVars(defaultPalette, wideTypo);
    expect(vars2["--editor-content-max-width"]).toBe("85ch");

    const fullTypo: TypographySettings = { ...defaultTypography, contentWidth: "full" };
    const vars3 = applyCssVars(defaultPalette, fullTypo);
    expect(vars3["--editor-content-max-width"]).toBe("none");
  });
});

// ===========================================================================
// clearCssVars
// ===========================================================================

describe("clearCssVars", () => {
  const defaultPalette = BUILTIN_PRESETS.default.colors.light;
  const defaultTypography: TypographySettings = {
    fontFamily: "inter",
    baseFontSize: 16,
    lineHeight: 1.6,
    contentWidth: "medium",
  };

  it("removes all dynamic CSS variables", () => {
    applyCssVars(defaultPalette, defaultTypography);
    const style = document.documentElement.style;
    expect(style.getPropertyValue("--background")).toBeTruthy();

    clearCssVars();
    expect(style.getPropertyValue("--background")).toBe("");
    expect(style.getPropertyValue("--editor-font-family")).toBe("");
    expect(style.getPropertyValue("--editor-font-size")).toBe("");
    expect(style.getPropertyValue("--editor-line-height")).toBe("");
    expect(style.getPropertyValue("--editor-content-max-width")).toBe("");
  });

  it("skips clearing in preview mode", () => {
    applyCssVars(defaultPalette, defaultTypography);
    (globalThis as any).__diaryx_preview = true;

    clearCssVars();
    // Variables should still be set
    expect(document.documentElement.style.getPropertyValue("--background")).toBeTruthy();
  });
});

// ===========================================================================
// cacheVarsForFouc / clearVarsCache
// ===========================================================================

describe("cacheVarsForFouc", () => {
  it("persists vars to localStorage", () => {
    const vars = { "--background": "oklch(1 0 0)", "--foreground": "oklch(0 0 0)" };
    cacheVarsForFouc(vars);

    const stored = localStorage.getItem("diaryx-appearance-vars");
    expect(stored).toBeTruthy();
    expect(JSON.parse(stored!)).toEqual(vars);
  });

  it("handles localStorage quota errors gracefully", () => {
    // Mock localStorage.setItem to throw
    const origSetItem = localStorage.setItem.bind(localStorage);
    localStorage.setItem = () => {
      throw new DOMException("QuotaExceededError");
    };

    // Should not throw
    expect(() => cacheVarsForFouc({ "--bg": "val" })).not.toThrow();
    localStorage.setItem = origSetItem;
  });
});

describe("clearVarsCache", () => {
  it("removes the FOUC cache from localStorage", () => {
    localStorage.setItem("diaryx-appearance-vars", "{}");
    clearVarsCache();
    expect(localStorage.getItem("diaryx-appearance-vars")).toBeNull();
  });
});

// ===========================================================================
// resolveEffectivePalette
// ===========================================================================

describe("resolveEffectivePalette", () => {
  const theme = BUILTIN_PRESETS.default;

  it("returns the light palette in light mode", () => {
    const palette = resolveEffectivePalette(theme, "light", null);
    expect(palette).toEqual(theme.colors.light);
  });

  it("returns the dark palette in dark mode", () => {
    const palette = resolveEffectivePalette(theme, "dark", null);
    expect(palette).toEqual(theme.colors.dark);
  });

  it("applies accent hue when provided", () => {
    const palette = resolveEffectivePalette(theme, "light", 120);
    expect(palette.primary).toContain("120");
    // Destructive unchanged
    expect(palette.destructive).toBe(theme.colors.light.destructive);
  });

  it("does not apply accent hue when null", () => {
    const palette = resolveEffectivePalette(theme, "light", null);
    expect(palette.primary).toBe(theme.colors.light.primary);
  });
});

// ===========================================================================
// DEFAULT_APPEARANCE
// ===========================================================================

describe("DEFAULT_APPEARANCE", () => {
  it("has expected default values", () => {
    expect(DEFAULT_APPEARANCE.presetId).toBe("default");
    expect(DEFAULT_APPEARANCE.accentHue).toBeNull();
    expect(DEFAULT_APPEARANCE.typographyPresetId).toBe("default");
    expect(DEFAULT_APPEARANCE.typographyOverrides).toEqual({});
    expect(DEFAULT_APPEARANCE.highContrastEditor).toBe(false);
  });
});
