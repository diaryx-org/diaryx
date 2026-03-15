/**
 * Built-in theme presets.
 *
 * "default" matches the existing app.css values exactly — selecting it
 * with no accent override produces zero visual change.
 */

import type { ThemeDefinition, TypographyDefinition } from "./appearance.types";

// ---------------------------------------------------------------------------
// Default (current app.css values verbatim)
// ---------------------------------------------------------------------------

const defaultPreset: ThemeDefinition = {
  id: "default",
  name: "Default",
  description: "The original Diaryx look.",
  version: 1,
  colors: {
    light: {
      background: "oklch(1 0 0)",
      foreground: "oklch(0.129 0.042 264.695)",
      card: "oklch(1 0 0)",
      "card-foreground": "oklch(0.129 0.042 264.695)",
      popover: "oklch(1 0 0)",
      "popover-foreground": "oklch(0.129 0.042 264.695)",
      primary: "oklch(0.208 0.042 265.755)",
      "primary-foreground": "oklch(0.984 0.003 247.858)",
      secondary: "oklch(0.968 0.007 247.896)",
      "secondary-foreground": "oklch(0.208 0.042 265.755)",
      muted: "oklch(0.968 0.007 247.896)",
      "muted-foreground": "oklch(0.554 0.046 257.417)",
      accent: "oklch(0.968 0.007 247.896)",
      "accent-foreground": "oklch(0.208 0.042 265.755)",
      destructive: "oklch(0.577 0.245 27.325)",
      border: "oklch(0.929 0.013 255.508)",
      input: "oklch(0.929 0.013 255.508)",
      ring: "oklch(0.704 0.04 256.788)",
      sidebar: "oklch(0.984 0.003 247.858)",
      "sidebar-foreground": "oklch(0.129 0.042 264.695)",
      "sidebar-primary": "oklch(0.208 0.042 265.755)",
      "sidebar-primary-foreground": "oklch(0.984 0.003 247.858)",
      "sidebar-accent": "oklch(0.968 0.007 247.896)",
      "sidebar-accent-foreground": "oklch(0.208 0.042 265.755)",
      "sidebar-border": "oklch(0.929 0.013 255.508)",
      "sidebar-ring": "oklch(0.704 0.04 256.788)",
    },
    dark: {
      background: "oklch(0.129 0.042 264.695)",
      foreground: "oklch(0.984 0.003 247.858)",
      card: "oklch(0.208 0.042 265.755)",
      "card-foreground": "oklch(0.984 0.003 247.858)",
      popover: "oklch(0.208 0.042 265.755)",
      "popover-foreground": "oklch(0.984 0.003 247.858)",
      primary: "oklch(0.929 0.013 255.508)",
      "primary-foreground": "oklch(0.208 0.042 265.755)",
      secondary: "oklch(0.279 0.041 260.031)",
      "secondary-foreground": "oklch(0.984 0.003 247.858)",
      muted: "oklch(0.279 0.041 260.031)",
      "muted-foreground": "oklch(0.704 0.04 256.788)",
      accent: "oklch(0.279 0.041 260.031)",
      "accent-foreground": "oklch(0.984 0.003 247.858)",
      destructive: "oklch(0.704 0.191 22.216)",
      border: "oklch(1 0 0 / 10%)",
      input: "oklch(1 0 0 / 15%)",
      ring: "oklch(0.551 0.027 264.364)",
      sidebar: "oklch(0.208 0.042 265.755)",
      "sidebar-foreground": "oklch(0.984 0.003 247.858)",
      "sidebar-primary": "oklch(0.488 0.243 264.376)",
      "sidebar-primary-foreground": "oklch(0.984 0.003 247.858)",
      "sidebar-accent": "oklch(0.279 0.041 260.031)",
      "sidebar-accent-foreground": "oklch(0.984 0.003 247.858)",
      "sidebar-border": "oklch(1 0 0 / 10%)",
      "sidebar-ring": "oklch(0.551 0.027 264.364)",
    },
  },
};

// ---------------------------------------------------------------------------
// Sepia — warm, paper-like tones
// ---------------------------------------------------------------------------

const sepiaPreset: ThemeDefinition = {
  id: "sepia",
  name: "Sepia",
  description: "Warm paper tones for comfortable reading.",
  version: 1,
  colors: {
    light: {
      background: "oklch(0.96 0.035 78)",
      foreground: "oklch(0.22 0.05 50)",
      card: "oklch(0.95 0.04 75)",
      "card-foreground": "oklch(0.22 0.05 50)",
      popover: "oklch(0.96 0.035 78)",
      "popover-foreground": "oklch(0.22 0.05 50)",
      primary: "oklch(0.42 0.12 50)",
      "primary-foreground": "oklch(0.96 0.035 78)",
      secondary: "oklch(0.91 0.04 72)",
      "secondary-foreground": "oklch(0.28 0.06 50)",
      muted: "oklch(0.92 0.035 74)",
      "muted-foreground": "oklch(0.52 0.05 58)",
      accent: "oklch(0.85 0.08 70)",
      "accent-foreground": "oklch(0.28 0.06 50)",
      destructive: "oklch(0.577 0.245 27.325)",
      border: "oklch(0.86 0.04 72)",
      input: "oklch(0.86 0.04 72)",
      ring: "oklch(0.55 0.10 50)",
      sidebar: "oklch(0.93 0.04 75)",
      "sidebar-foreground": "oklch(0.22 0.05 50)",
      "sidebar-primary": "oklch(0.42 0.12 50)",
      "sidebar-primary-foreground": "oklch(0.96 0.035 78)",
      "sidebar-accent": "oklch(0.88 0.06 70)",
      "sidebar-accent-foreground": "oklch(0.28 0.06 50)",
      "sidebar-border": "oklch(0.86 0.04 72)",
      "sidebar-ring": "oklch(0.55 0.10 50)",
    },
    dark: {
      background: "oklch(0.18 0.04 55)",
      foreground: "oklch(0.90 0.03 75)",
      card: "oklch(0.22 0.045 53)",
      "card-foreground": "oklch(0.90 0.03 75)",
      popover: "oklch(0.22 0.045 53)",
      "popover-foreground": "oklch(0.90 0.03 75)",
      primary: "oklch(0.78 0.10 60)",
      "primary-foreground": "oklch(0.18 0.04 55)",
      secondary: "oklch(0.28 0.04 53)",
      "secondary-foreground": "oklch(0.90 0.03 75)",
      muted: "oklch(0.28 0.035 55)",
      "muted-foreground": "oklch(0.62 0.04 62)",
      accent: "oklch(0.33 0.06 55)",
      "accent-foreground": "oklch(0.90 0.03 75)",
      destructive: "oklch(0.704 0.191 22.216)",
      border: "oklch(0.34 0.04 53)",
      input: "oklch(0.34 0.04 53)",
      ring: "oklch(0.58 0.08 55)",
      sidebar: "oklch(0.16 0.04 55)",
      "sidebar-foreground": "oklch(0.90 0.03 75)",
      "sidebar-primary": "oklch(0.75 0.10 60)",
      "sidebar-primary-foreground": "oklch(0.96 0.035 78)",
      "sidebar-accent": "oklch(0.30 0.05 55)",
      "sidebar-accent-foreground": "oklch(0.90 0.03 75)",
      "sidebar-border": "oklch(0.34 0.04 53)",
      "sidebar-ring": "oklch(0.58 0.08 55)",
    },
  },
};

// ---------------------------------------------------------------------------
// Nord — cool blue/grey arctic palette
// ---------------------------------------------------------------------------

const nordPreset: ThemeDefinition = {
  id: "nord",
  name: "Nord",
  description: "Cool arctic blues and soft contrasts.",
  author: "Arctic Ice Studio",
  version: 1,
  colors: {
    light: {
      background: "oklch(0.96 0.02 230)",
      foreground: "oklch(0.25 0.04 240)",
      card: "oklch(0.94 0.025 228)",
      "card-foreground": "oklch(0.25 0.04 240)",
      popover: "oklch(0.96 0.02 230)",
      "popover-foreground": "oklch(0.25 0.04 240)",
      primary: "oklch(0.55 0.14 240)",
      "primary-foreground": "oklch(0.96 0.02 230)",
      secondary: "oklch(0.90 0.03 228)",
      "secondary-foreground": "oklch(0.28 0.05 240)",
      muted: "oklch(0.91 0.025 230)",
      "muted-foreground": "oklch(0.52 0.04 235)",
      accent: "oklch(0.82 0.08 170)",
      "accent-foreground": "oklch(0.25 0.04 240)",
      destructive: "oklch(0.6 0.2 25)",
      border: "oklch(0.86 0.03 228)",
      input: "oklch(0.86 0.03 228)",
      ring: "oklch(0.55 0.14 240)",
      sidebar: "oklch(0.93 0.025 228)",
      "sidebar-foreground": "oklch(0.25 0.04 240)",
      "sidebar-primary": "oklch(0.55 0.14 240)",
      "sidebar-primary-foreground": "oklch(0.96 0.02 230)",
      "sidebar-accent": "oklch(0.85 0.06 170)",
      "sidebar-accent-foreground": "oklch(0.28 0.05 240)",
      "sidebar-border": "oklch(0.86 0.03 228)",
      "sidebar-ring": "oklch(0.55 0.14 240)",
    },
    dark: {
      background: "oklch(0.22 0.04 240)",
      foreground: "oklch(0.91 0.02 225)",
      card: "oklch(0.26 0.045 238)",
      "card-foreground": "oklch(0.91 0.02 225)",
      popover: "oklch(0.26 0.045 238)",
      "popover-foreground": "oklch(0.91 0.02 225)",
      primary: "oklch(0.72 0.12 235)",
      "primary-foreground": "oklch(0.22 0.04 240)",
      secondary: "oklch(0.30 0.04 238)",
      "secondary-foreground": "oklch(0.91 0.02 225)",
      muted: "oklch(0.30 0.035 238)",
      "muted-foreground": "oklch(0.63 0.035 232)",
      accent: "oklch(0.35 0.07 170)",
      "accent-foreground": "oklch(0.78 0.08 170)",
      destructive: "oklch(0.65 0.18 20)",
      border: "oklch(0.34 0.04 238)",
      input: "oklch(0.36 0.04 238)",
      ring: "oklch(0.60 0.10 235)",
      sidebar: "oklch(0.20 0.04 240)",
      "sidebar-foreground": "oklch(0.91 0.02 225)",
      "sidebar-primary": "oklch(0.72 0.12 235)",
      "sidebar-primary-foreground": "oklch(0.91 0.02 225)",
      "sidebar-accent": "oklch(0.32 0.06 170)",
      "sidebar-accent-foreground": "oklch(0.78 0.08 170)",
      "sidebar-border": "oklch(0.34 0.04 238)",
      "sidebar-ring": "oklch(0.60 0.10 235)",
    },
  },
};

// ---------------------------------------------------------------------------
// Rosé Pine — muted rose/pink/gold on dark plum
// ---------------------------------------------------------------------------

const rosePinePreset: ThemeDefinition = {
  id: "rose-pine",
  name: "Rosé Pine",
  description: "Soho vibes with muted rose and gold.",
  author: "Rosé Pine",
  version: 1,
  colors: {
    light: {
      background: "oklch(0.95 0.025 315)",
      foreground: "oklch(0.30 0.05 300)",
      card: "oklch(0.93 0.03 312)",
      "card-foreground": "oklch(0.30 0.05 300)",
      popover: "oklch(0.95 0.025 315)",
      "popover-foreground": "oklch(0.30 0.05 300)",
      primary: "oklch(0.58 0.18 335)",
      "primary-foreground": "oklch(0.97 0.015 315)",
      secondary: "oklch(0.90 0.03 310)",
      "secondary-foreground": "oklch(0.33 0.05 300)",
      muted: "oklch(0.91 0.025 312)",
      "muted-foreground": "oklch(0.53 0.04 300)",
      accent: "oklch(0.82 0.08 80)",
      "accent-foreground": "oklch(0.30 0.05 300)",
      destructive: "oklch(0.6 0.2 25)",
      border: "oklch(0.85 0.035 310)",
      input: "oklch(0.85 0.035 310)",
      ring: "oklch(0.58 0.18 335)",
      sidebar: "oklch(0.92 0.03 312)",
      "sidebar-foreground": "oklch(0.30 0.05 300)",
      "sidebar-primary": "oklch(0.58 0.18 335)",
      "sidebar-primary-foreground": "oklch(0.97 0.015 315)",
      "sidebar-accent": "oklch(0.88 0.04 320)",
      "sidebar-accent-foreground": "oklch(0.33 0.05 300)",
      "sidebar-border": "oklch(0.85 0.035 310)",
      "sidebar-ring": "oklch(0.58 0.18 335)",
    },
    dark: {
      background: "oklch(0.18 0.055 300)",
      foreground: "oklch(0.88 0.025 315)",
      card: "oklch(0.22 0.055 298)",
      "card-foreground": "oklch(0.88 0.025 315)",
      popover: "oklch(0.22 0.055 298)",
      "popover-foreground": "oklch(0.88 0.025 315)",
      primary: "oklch(0.72 0.16 335)",
      "primary-foreground": "oklch(0.18 0.055 300)",
      secondary: "oklch(0.26 0.05 298)",
      "secondary-foreground": "oklch(0.88 0.025 315)",
      muted: "oklch(0.27 0.045 300)",
      "muted-foreground": "oklch(0.60 0.04 308)",
      accent: "oklch(0.35 0.07 80)",
      "accent-foreground": "oklch(0.78 0.08 80)",
      destructive: "oklch(0.65 0.18 20)",
      border: "oklch(0.31 0.05 298)",
      input: "oklch(0.33 0.05 298)",
      ring: "oklch(0.60 0.14 335)",
      sidebar: "oklch(0.16 0.055 300)",
      "sidebar-foreground": "oklch(0.88 0.025 315)",
      "sidebar-primary": "oklch(0.72 0.16 335)",
      "sidebar-primary-foreground": "oklch(0.88 0.025 315)",
      "sidebar-accent": "oklch(0.22 0.055 320)",
      "sidebar-accent-foreground": "oklch(0.88 0.025 315)",
      "sidebar-border": "oklch(0.31 0.05 298)",
      "sidebar-ring": "oklch(0.60 0.14 335)",
    },
  },
};

// ---------------------------------------------------------------------------
// Exports
// ---------------------------------------------------------------------------

/** Ordered list for built-in presets. */
export const BUILTIN_PRESET_LIST: ThemeDefinition[] = [
  defaultPreset,
  sepiaPreset,
  nordPreset,
  rosePinePreset,
];

/** All built-in presets, keyed by id for O(1) lookup. */
export const BUILTIN_PRESETS: Readonly<Record<string, ThemeDefinition>> =
  Object.freeze(
    Object.fromEntries(
      BUILTIN_PRESET_LIST.map((preset) => [preset.id, preset]),
    ) as Record<string, ThemeDefinition>,
  );

// Backward-compatible aliases for existing imports.
export const PRESETS = BUILTIN_PRESETS;
export const PRESET_LIST = BUILTIN_PRESET_LIST;

// ---------------------------------------------------------------------------
// Built-in typography presets
// ---------------------------------------------------------------------------

const defaultTypographyPreset: TypographyDefinition = {
  id: "default",
  name: "Default",
  description: "Balanced defaults tuned for everyday writing.",
  version: 1,
  settings: {
    fontFamily: "inter",
    baseFontSize: 16,
    lineHeight: 1.6,
    contentWidth: "medium",
  },
};

const editorialTypographyPreset: TypographyDefinition = {
  id: "editorial-serif",
  name: "Editorial Serif",
  description: "Book-like reading with serif body text and comfortable spacing.",
  version: 1,
  settings: {
    fontFamily: "serif",
    baseFontSize: 18,
    lineHeight: 1.8,
    contentWidth: "narrow",
  },
};

const compactTypographyPreset: TypographyDefinition = {
  id: "compact-system",
  name: "Compact System",
  description: "Dense UI-adjacent writing with tighter rhythm.",
  version: 1,
  settings: {
    fontFamily: "system",
    baseFontSize: 15,
    lineHeight: 1.5,
    contentWidth: "wide",
  },
};

const codeNotebookTypographyPreset: TypographyDefinition = {
  id: "code-notebook",
  name: "Code Notebook",
  description: "Monospaced notes for logs, commands, and technical journals.",
  version: 1,
  settings: {
    fontFamily: "mono",
    baseFontSize: 15,
    lineHeight: 1.6,
    contentWidth: "full",
  },
};

export const BUILTIN_TYPOGRAPHY_PRESET_LIST: TypographyDefinition[] = [
  defaultTypographyPreset,
  editorialTypographyPreset,
  compactTypographyPreset,
  codeNotebookTypographyPreset,
];

export const BUILTIN_TYPOGRAPHY_PRESETS: Readonly<
  Record<string, TypographyDefinition>
> = Object.freeze(
  Object.fromEntries(
    BUILTIN_TYPOGRAPHY_PRESET_LIST.map((preset) => [preset.id, preset]),
  ) as Record<string, TypographyDefinition>,
);

// Backward-compatible aliases for existing imports.
export const TYPOGRAPHY_PRESETS = BUILTIN_TYPOGRAPHY_PRESETS;
export const TYPOGRAPHY_PRESET_LIST = BUILTIN_TYPOGRAPHY_PRESET_LIST;
