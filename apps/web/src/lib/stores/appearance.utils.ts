/**
 * Utility helpers for the appearance / theme system.
 *
 * - OKLch parsing & accent hue shifting
 * - CSS variable application & clearing
 * - Pre-computed vars cache for FOUC prevention
 */

import type {
  ThemeColorPalette,
  ThemeColorKey,
  ThemeDefinition,
  UserAppearance,
} from "./appearance.types";
import { FONT_FAMILY_MAP, CONTENT_WIDTH_MAP } from "./appearance.types";

// ---------------------------------------------------------------------------
// OKLch helpers
// ---------------------------------------------------------------------------

const OKLCH_RE = /oklch\(\s*([\d.]+)\s+([\d.]+)\s+([\d.]+)\s*\)/;
const OKLCH_ALPHA_RE = /oklch\(\s*([\d.]+)\s+([\d.]+)\s+([\d.]+)\s*\/\s*[\d.]+%?\s*\)/;

interface OklchComponents {
  L: number;
  C: number;
  H: number;
}

/** Parse an OKLch CSS string into its L, C, H components. Returns null for non-oklch values. */
export function parseOklch(value: string): OklchComponents | null {
  const m = OKLCH_RE.exec(value) ?? OKLCH_ALPHA_RE.exec(value);
  if (!m) return null;
  return { L: parseFloat(m[1]), C: parseFloat(m[2]), H: parseFloat(m[3]) };
}

/** Rebuild an oklch() string from components. */
export function toOklchString(c: OklchComponents): string {
  return `oklch(${c.L} ${c.C} ${c.H})`;
}

/** Replace the hue component of an OKLch color string, preserving L, C, and any alpha. */
export function shiftHue(value: string, newHue: number): string {
  // Handle alpha variant
  const alphaMatch = value.match(
    /oklch\(\s*([\d.]+)\s+([\d.]+)\s+([\d.]+)\s*(\/\s*[\d.]+%?\s*)\)/,
  );
  if (alphaMatch) {
    return `oklch(${alphaMatch[1]} ${alphaMatch[2]} ${newHue}${alphaMatch[4]})`;
  }
  // Standard variant
  const match = OKLCH_RE.exec(value);
  if (!match) return value;
  return `oklch(${match[1]} ${match[2]} ${newHue})`;
}

// ---------------------------------------------------------------------------
// Accent hue override
// ---------------------------------------------------------------------------

/**
 * Keys that should be shifted when the user applies an accent hue override.
 * `destructive` is intentionally excluded — it must stay red for UX.
 */
const ACCENT_KEYS: ThemeColorKey[] = [
  "primary",
  "primary-foreground",
  "ring",
  "accent",
  "sidebar-primary",
  "sidebar-ring",
];

/**
 * Given a palette and an accent hue (0–360), return a copy with the hue-shifted
 * accent colors.
 */
export function applyAccentHue(
  palette: ThemeColorPalette,
  hue: number,
): ThemeColorPalette {
  const out = { ...palette };
  for (const key of ACCENT_KEYS) {
    out[key] = shiftHue(palette[key], hue);
  }
  return out;
}

// ---------------------------------------------------------------------------
// CSS variable application
// ---------------------------------------------------------------------------

/** All theme color keys (for iteration). */
const ALL_COLOR_KEYS: ThemeColorKey[] = [
  "background", "foreground",
  "card", "card-foreground",
  "popover", "popover-foreground",
  "primary", "primary-foreground",
  "secondary", "secondary-foreground",
  "muted", "muted-foreground",
  "accent", "accent-foreground",
  "destructive",
  "border", "input", "ring",
  "sidebar", "sidebar-foreground",
  "sidebar-primary", "sidebar-primary-foreground",
  "sidebar-accent", "sidebar-accent-foreground",
  "sidebar-border", "sidebar-ring",
];

/**
 * Apply a full set of CSS variables to `document.documentElement.style`.
 * Returns a flat Record of `--var: value` pairs (useful for caching).
 */
export function applyCssVars(
  palette: ThemeColorPalette,
  typography: UserAppearance["typography"],
  layout: UserAppearance["layout"],
): Record<string, string> {
  const vars: Record<string, string> = {};
  const style = document.documentElement.style;

  // Color palette
  for (const key of ALL_COLOR_KEYS) {
    const prop = `--${key}`;
    const val = palette[key];
    style.setProperty(prop, val);
    vars[prop] = val;
  }

  // Typography
  const fontFamily = FONT_FAMILY_MAP[typography.fontFamily];
  style.setProperty("--editor-font-family", fontFamily);
  vars["--editor-font-family"] = fontFamily;

  const fontSize = `${typography.baseFontSize}px`;
  style.setProperty("--editor-font-size", fontSize);
  vars["--editor-font-size"] = fontSize;

  const lineHeight = String(typography.lineHeight);
  style.setProperty("--editor-line-height", lineHeight);
  vars["--editor-line-height"] = lineHeight;

  // Layout
  const maxWidth = CONTENT_WIDTH_MAP[layout.contentWidth];
  style.setProperty("--editor-content-max-width", maxWidth);
  vars["--editor-content-max-width"] = maxWidth;

  return vars;
}

/**
 * Remove all dynamic CSS variables so static app.css values take over.
 */
export function clearCssVars(): void {
  const style = document.documentElement.style;

  for (const key of ALL_COLOR_KEYS) {
    style.removeProperty(`--${key}`);
  }
  style.removeProperty("--editor-font-family");
  style.removeProperty("--editor-font-size");
  style.removeProperty("--editor-line-height");
  style.removeProperty("--editor-content-max-width");
}

// ---------------------------------------------------------------------------
// FOUC-prevention cache
// ---------------------------------------------------------------------------

const VARS_CACHE_KEY = "diaryx-appearance-vars";

/** Persist pre-computed CSS vars to localStorage for the inline FOUC script. */
export function cacheVarsForFouc(vars: Record<string, string>): void {
  try {
    localStorage.setItem(VARS_CACHE_KEY, JSON.stringify(vars));
  } catch {
    // Ignore quota errors
  }
}

/** Clear the FOUC cache (used when resetting to default). */
export function clearVarsCache(): void {
  localStorage.removeItem(VARS_CACHE_KEY);
}

// ---------------------------------------------------------------------------
// Resolve effective palette
// ---------------------------------------------------------------------------

/**
 * Given the current theme definition, resolved mode (light/dark), and optional
 * accent hue, return the palette to apply.
 */
export function resolveEffectivePalette(
  theme: ThemeDefinition,
  mode: "light" | "dark",
  accentHue: number | null,
): ThemeColorPalette {
  let palette = mode === "dark" ? theme.colors.dark : theme.colors.light;
  if (accentHue !== null) {
    palette = applyAccentHue(palette, accentHue);
  }
  return palette;
}

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

export const DEFAULT_APPEARANCE: UserAppearance = {
  presetId: "default",
  accentHue: null,
  typography: {
    fontFamily: "inter",
    baseFontSize: 16,
    lineHeight: 1.6,
  },
  layout: {
    contentWidth: "medium",
  },
};
