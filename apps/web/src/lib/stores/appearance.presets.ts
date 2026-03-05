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
      background: "oklch(0.97 0.015 80)",
      foreground: "oklch(0.25 0.04 55)",
      card: "oklch(0.96 0.018 78)",
      "card-foreground": "oklch(0.25 0.04 55)",
      popover: "oklch(0.97 0.015 80)",
      "popover-foreground": "oklch(0.25 0.04 55)",
      primary: "oklch(0.45 0.08 55)",
      "primary-foreground": "oklch(0.97 0.015 80)",
      secondary: "oklch(0.93 0.02 75)",
      "secondary-foreground": "oklch(0.3 0.05 55)",
      muted: "oklch(0.93 0.02 75)",
      "muted-foreground": "oklch(0.55 0.04 60)",
      accent: "oklch(0.93 0.02 75)",
      "accent-foreground": "oklch(0.3 0.05 55)",
      destructive: "oklch(0.577 0.245 27.325)",
      border: "oklch(0.88 0.025 75)",
      input: "oklch(0.88 0.025 75)",
      ring: "oklch(0.6 0.06 55)",
      sidebar: "oklch(0.95 0.02 78)",
      "sidebar-foreground": "oklch(0.25 0.04 55)",
      "sidebar-primary": "oklch(0.45 0.08 55)",
      "sidebar-primary-foreground": "oklch(0.97 0.015 80)",
      "sidebar-accent": "oklch(0.93 0.02 75)",
      "sidebar-accent-foreground": "oklch(0.3 0.05 55)",
      "sidebar-border": "oklch(0.88 0.025 75)",
      "sidebar-ring": "oklch(0.6 0.06 55)",
    },
    dark: {
      background: "oklch(0.2 0.025 60)",
      foreground: "oklch(0.9 0.02 75)",
      card: "oklch(0.25 0.03 58)",
      "card-foreground": "oklch(0.9 0.02 75)",
      popover: "oklch(0.25 0.03 58)",
      "popover-foreground": "oklch(0.9 0.02 75)",
      primary: "oklch(0.82 0.06 65)",
      "primary-foreground": "oklch(0.2 0.025 60)",
      secondary: "oklch(0.3 0.03 58)",
      "secondary-foreground": "oklch(0.9 0.02 75)",
      muted: "oklch(0.3 0.03 58)",
      "muted-foreground": "oklch(0.65 0.03 65)",
      accent: "oklch(0.3 0.03 58)",
      "accent-foreground": "oklch(0.9 0.02 75)",
      destructive: "oklch(0.704 0.191 22.216)",
      border: "oklch(0.35 0.03 58)",
      input: "oklch(0.35 0.03 58)",
      ring: "oklch(0.55 0.04 60)",
      sidebar: "oklch(0.23 0.028 58)",
      "sidebar-foreground": "oklch(0.9 0.02 75)",
      "sidebar-primary": "oklch(0.75 0.08 65)",
      "sidebar-primary-foreground": "oklch(0.97 0.015 80)",
      "sidebar-accent": "oklch(0.3 0.03 58)",
      "sidebar-accent-foreground": "oklch(0.9 0.02 75)",
      "sidebar-border": "oklch(0.35 0.03 58)",
      "sidebar-ring": "oklch(0.55 0.04 60)",
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
      background: "oklch(0.97 0.005 230)",
      foreground: "oklch(0.27 0.03 240)",
      card: "oklch(0.95 0.008 230)",
      "card-foreground": "oklch(0.27 0.03 240)",
      popover: "oklch(0.97 0.005 230)",
      "popover-foreground": "oklch(0.27 0.03 240)",
      primary: "oklch(0.55 0.1 240)",
      "primary-foreground": "oklch(0.97 0.005 230)",
      secondary: "oklch(0.92 0.01 230)",
      "secondary-foreground": "oklch(0.3 0.04 240)",
      muted: "oklch(0.92 0.01 230)",
      "muted-foreground": "oklch(0.55 0.03 235)",
      accent: "oklch(0.92 0.01 230)",
      "accent-foreground": "oklch(0.3 0.04 240)",
      destructive: "oklch(0.6 0.2 25)",
      border: "oklch(0.87 0.015 230)",
      input: "oklch(0.87 0.015 230)",
      ring: "oklch(0.55 0.1 240)",
      sidebar: "oklch(0.95 0.008 230)",
      "sidebar-foreground": "oklch(0.27 0.03 240)",
      "sidebar-primary": "oklch(0.55 0.1 240)",
      "sidebar-primary-foreground": "oklch(0.97 0.005 230)",
      "sidebar-accent": "oklch(0.92 0.01 230)",
      "sidebar-accent-foreground": "oklch(0.3 0.04 240)",
      "sidebar-border": "oklch(0.87 0.015 230)",
      "sidebar-ring": "oklch(0.55 0.1 240)",
    },
    dark: {
      background: "oklch(0.24 0.025 240)",
      foreground: "oklch(0.91 0.01 230)",
      card: "oklch(0.28 0.03 238)",
      "card-foreground": "oklch(0.91 0.01 230)",
      popover: "oklch(0.28 0.03 238)",
      "popover-foreground": "oklch(0.91 0.01 230)",
      primary: "oklch(0.72 0.1 240)",
      "primary-foreground": "oklch(0.24 0.025 240)",
      secondary: "oklch(0.32 0.03 238)",
      "secondary-foreground": "oklch(0.91 0.01 230)",
      muted: "oklch(0.32 0.03 238)",
      "muted-foreground": "oklch(0.65 0.025 235)",
      accent: "oklch(0.32 0.03 238)",
      "accent-foreground": "oklch(0.91 0.01 230)",
      destructive: "oklch(0.65 0.18 20)",
      border: "oklch(0.35 0.03 238)",
      input: "oklch(0.38 0.03 238)",
      ring: "oklch(0.55 0.07 240)",
      sidebar: "oklch(0.22 0.025 240)",
      "sidebar-foreground": "oklch(0.91 0.01 230)",
      "sidebar-primary": "oklch(0.72 0.1 240)",
      "sidebar-primary-foreground": "oklch(0.91 0.01 230)",
      "sidebar-accent": "oklch(0.32 0.03 238)",
      "sidebar-accent-foreground": "oklch(0.91 0.01 230)",
      "sidebar-border": "oklch(0.35 0.03 238)",
      "sidebar-ring": "oklch(0.55 0.07 240)",
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
      background: "oklch(0.96 0.012 310)",
      foreground: "oklch(0.32 0.04 300)",
      card: "oklch(0.94 0.015 308)",
      "card-foreground": "oklch(0.32 0.04 300)",
      popover: "oklch(0.96 0.012 310)",
      "popover-foreground": "oklch(0.32 0.04 300)",
      primary: "oklch(0.55 0.14 330)",
      "primary-foreground": "oklch(0.97 0.01 310)",
      secondary: "oklch(0.92 0.015 308)",
      "secondary-foreground": "oklch(0.35 0.04 300)",
      muted: "oklch(0.92 0.015 308)",
      "muted-foreground": "oklch(0.55 0.03 300)",
      accent: "oklch(0.92 0.015 308)",
      "accent-foreground": "oklch(0.35 0.04 300)",
      destructive: "oklch(0.6 0.2 25)",
      border: "oklch(0.87 0.02 308)",
      input: "oklch(0.87 0.02 308)",
      ring: "oklch(0.55 0.14 330)",
      sidebar: "oklch(0.94 0.015 308)",
      "sidebar-foreground": "oklch(0.32 0.04 300)",
      "sidebar-primary": "oklch(0.55 0.14 330)",
      "sidebar-primary-foreground": "oklch(0.97 0.01 310)",
      "sidebar-accent": "oklch(0.92 0.015 308)",
      "sidebar-accent-foreground": "oklch(0.35 0.04 300)",
      "sidebar-border": "oklch(0.87 0.02 308)",
      "sidebar-ring": "oklch(0.55 0.14 330)",
    },
    dark: {
      background: "oklch(0.2 0.04 300)",
      foreground: "oklch(0.88 0.015 310)",
      card: "oklch(0.24 0.04 298)",
      "card-foreground": "oklch(0.88 0.015 310)",
      popover: "oklch(0.24 0.04 298)",
      "popover-foreground": "oklch(0.88 0.015 310)",
      primary: "oklch(0.72 0.12 330)",
      "primary-foreground": "oklch(0.2 0.04 300)",
      secondary: "oklch(0.28 0.04 298)",
      "secondary-foreground": "oklch(0.88 0.015 310)",
      muted: "oklch(0.28 0.04 298)",
      "muted-foreground": "oklch(0.62 0.03 305)",
      accent: "oklch(0.28 0.04 298)",
      "accent-foreground": "oklch(0.88 0.015 310)",
      destructive: "oklch(0.65 0.18 20)",
      border: "oklch(0.33 0.04 298)",
      input: "oklch(0.35 0.04 298)",
      ring: "oklch(0.55 0.1 330)",
      sidebar: "oklch(0.18 0.04 300)",
      "sidebar-foreground": "oklch(0.88 0.015 310)",
      "sidebar-primary": "oklch(0.72 0.12 330)",
      "sidebar-primary-foreground": "oklch(0.88 0.015 310)",
      "sidebar-accent": "oklch(0.28 0.04 298)",
      "sidebar-accent-foreground": "oklch(0.88 0.015 310)",
      "sidebar-border": "oklch(0.33 0.04 298)",
      "sidebar-ring": "oklch(0.55 0.1 330)",
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
