/**
 * Type definitions for the custom theme / appearance system.
 *
 * Light/dark/system mode stays in theme.svelte.ts — appearance is orthogonal.
 */

// ---------------------------------------------------------------------------
// Color palette
// ---------------------------------------------------------------------------

/** All CSS variable keys that a theme palette defines. Values are OKLch strings. */
export interface ThemeColorPalette {
  background: string;
  foreground: string;
  card: string;
  "card-foreground": string;
  popover: string;
  "popover-foreground": string;
  primary: string;
  "primary-foreground": string;
  secondary: string;
  "secondary-foreground": string;
  muted: string;
  "muted-foreground": string;
  accent: string;
  "accent-foreground": string;
  destructive: string;
  border: string;
  input: string;
  ring: string;
  sidebar: string;
  "sidebar-foreground": string;
  "sidebar-primary": string;
  "sidebar-primary-foreground": string;
  "sidebar-accent": string;
  "sidebar-accent-foreground": string;
  "sidebar-border": string;
  "sidebar-ring": string;
}

/** Every key in a ThemeColorPalette. */
export type ThemeColorKey = keyof ThemeColorPalette;

// ---------------------------------------------------------------------------
// Theme definition (preset or exported)
// ---------------------------------------------------------------------------

export interface ThemeDefinition {
  id: string;
  name: string;
  description?: string;
  author?: string;
  version: 1;
  colors: {
    light: ThemeColorPalette;
    dark: ThemeColorPalette;
  };
}

export type ThemeSource = "builtin" | "registry" | "local" | "bundle";

export interface ThemeSourceMetadata {
  source: ThemeSource;
  registryId?: string;
  fileName?: string;
  installedAt?: number;
}

export interface ThemeLibraryEntry {
  theme: ThemeDefinition;
  source: ThemeSourceMetadata;
}

export interface ThemeCatalogEntry {
  theme: ThemeDefinition;
  source: ThemeSourceMetadata;
  builtin: boolean;
}

// ---------------------------------------------------------------------------
// Typography definition (preset or exported)
// ---------------------------------------------------------------------------

export type FontFamily = "inter" | "system" | "serif" | "mono";
export type ContentWidth = "narrow" | "medium" | "wide" | "full";

export interface TypographySettings {
  fontFamily: FontFamily;
  baseFontSize: number;
  lineHeight: number;
  contentWidth: ContentWidth;
}

export interface TypographyDefinition {
  id: string;
  name: string;
  description?: string;
  author?: string;
  version: 1;
  settings: TypographySettings;
}

export type TypographySource = "builtin" | "registry" | "local" | "bundle";

export interface TypographySourceMetadata {
  source: TypographySource;
  registryId?: string;
  fileName?: string;
  installedAt?: number;
}

export interface TypographyLibraryEntry {
  typography: TypographyDefinition;
  source: TypographySourceMetadata;
}

export interface TypographyCatalogEntry {
  typography: TypographyDefinition;
  source: TypographySourceMetadata;
  builtin: boolean;
}

// ---------------------------------------------------------------------------
// User appearance (stored in localStorage)
// ---------------------------------------------------------------------------

export interface UserAppearance {
  presetId: string;
  accentHue: number | null;
  typographyPresetId: string;
  typographyOverrides: Partial<TypographySettings>;
}

// ---------------------------------------------------------------------------
// Export / import format (future sharing / gallery)
// ---------------------------------------------------------------------------

export interface ThemeExport {
  $schema: string;
  theme: ThemeDefinition;
}

export interface TypographyExport {
  $schema: string;
  typography: TypographyDefinition;
}

// ---------------------------------------------------------------------------
// CSS variable keys that the theme system also manages
// ---------------------------------------------------------------------------

/** Variables added for typography & layout, with their defaults. */
export const EDITOR_CSS_VARS = {
  "--editor-font-family": '"Inter", -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif',
  "--editor-font-size": "16px",
  "--editor-line-height": "1.6",
  "--editor-content-max-width": "65ch",
} as const;

/** Maps FontFamily choice to a CSS font-family value. */
export const FONT_FAMILY_MAP: Record<FontFamily, string> = {
  inter: '"Inter", -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif',
  system: '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif',
  serif: '"Georgia", "Times New Roman", serif',
  mono: '"SF Mono", Monaco, "Cascadia Code", "Fira Code", monospace',
};

/** Maps ContentWidth choice to a CSS max-width value. */
export const CONTENT_WIDTH_MAP: Record<ContentWidth, string> = {
  narrow: "55ch",
  medium: "65ch",
  wide: "85ch",
  full: "none",
};
