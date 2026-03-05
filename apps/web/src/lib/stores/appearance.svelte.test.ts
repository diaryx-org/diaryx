import { beforeEach, describe, expect, it, vi } from "vitest";

import { BUILTIN_PRESETS, BUILTIN_TYPOGRAPHY_PRESETS } from "./appearance.presets";
import { createAppearanceStore } from "./appearance.svelte";
import type { ThemeDefinition, TypographyDefinition } from "./appearance.types";

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
});

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
});
