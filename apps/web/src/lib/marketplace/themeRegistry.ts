import yaml from "js-yaml";

import type {
  ThemeColorKey,
  ThemeColorPalette,
  ThemeDefinition,
} from "$lib/stores/appearance.types";
import type { ThemeRegistryEntry } from "./types";

export interface ThemeRegistry {
  schema_version: 1;
  generated_at: string;
  themes: ThemeRegistryEntry[];
}

const TRUSTED_THEME_REGISTRY_URLS = [
  "https://cdn.diaryx.org/themes/registry.md",
] as const;

const DEFAULT_THEME_REGISTRY_URL = TRUSTED_THEME_REGISTRY_URLS[0];

const THEME_COLOR_KEYS: ThemeColorKey[] = [
  "background",
  "foreground",
  "card",
  "card-foreground",
  "popover",
  "popover-foreground",
  "primary",
  "primary-foreground",
  "secondary",
  "secondary-foreground",
  "muted",
  "muted-foreground",
  "accent",
  "accent-foreground",
  "destructive",
  "border",
  "input",
  "ring",
  "sidebar",
  "sidebar-foreground",
  "sidebar-primary",
  "sidebar-primary-foreground",
  "sidebar-accent",
  "sidebar-accent-foreground",
  "sidebar-border",
  "sidebar-ring",
];

let cachedRegistry: ThemeRegistry | null = null;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function readString(obj: Record<string, unknown>, key: string): string {
  const value = obj[key];
  if (typeof value !== "string" || value.length === 0) {
    throw new Error(
      `Theme registry validation error: '${key}' must be a non-empty string`,
    );
  }
  return value;
}

function readOptionalString(
  obj: Record<string, unknown>,
  key: string,
): string | null {
  const value = obj[key];
  if (value == null) return null;
  if (typeof value !== "string") {
    throw new Error(
      `Theme registry validation error: '${key}' must be a string or null`,
    );
  }
  return value;
}

function readStringArray(obj: Record<string, unknown>, key: string): string[] {
  const value = obj[key];
  if (!Array.isArray(value)) return [];
  if (value.some((v) => typeof v !== "string")) {
    throw new Error(
      `Theme registry validation error: '${key}' must be a string[]`,
    );
  }
  return value;
}

function parseMarkdownFrontmatter(text: string): {
  frontmatter: Record<string, unknown>;
  body: string;
} {
  const match = text.match(/^---\r?\n([\s\S]*?)\r?\n---\r?\n([\s\S]*)$/);
  if (!match) {
    throw new Error("Theme registry validation error: missing YAML frontmatter");
  }

  const frontmatter = yaml.load(match[1]) as Record<string, unknown>;
  if (!isRecord(frontmatter)) {
    throw new Error(
      "Theme registry validation error: frontmatter must be a YAML mapping",
    );
  }

  return { frontmatter, body: match[2] };
}

function validatePalette(
  input: unknown,
  key: "light" | "dark",
): ThemeColorPalette {
  if (!isRecord(input)) {
    throw new Error(`Theme registry validation error: theme.colors.${key} must be an object`);
  }

  const palette = {} as ThemeColorPalette;
  for (const colorKey of THEME_COLOR_KEYS) {
    const value = input[colorKey];
    if (typeof value !== "string" || value.length === 0) {
      throw new Error(
        `Theme registry validation error: theme.colors.${key}.${colorKey} must be a non-empty string`,
      );
    }
    palette[colorKey] = value;
  }

  return palette;
}

function validateThemeDefinition(input: unknown): ThemeDefinition {
  if (!isRecord(input)) {
    throw new Error("Theme registry validation error: theme must be an object");
  }

  const version = input.version;
  if (version !== 1) {
    throw new Error(
      `Theme registry validation error: theme.version must be 1 (received ${String(version)})`,
    );
  }

  const colors = input.colors;
  if (!isRecord(colors)) {
    throw new Error("Theme registry validation error: theme.colors must be an object");
  }

  return {
    id: readString(input, "id"),
    name: readString(input, "name"),
    description: readOptionalString(input, "description") ?? undefined,
    author: readOptionalString(input, "author") ?? undefined,
    version: 1,
    colors: {
      light: validatePalette(colors.light, "light"),
      dark: validatePalette(colors.dark, "dark"),
    },
  };
}

function readArtifact(
  input: Record<string, unknown>,
): ThemeRegistryEntry["artifact"] {
  const artifactRaw = input.artifact;
  if (artifactRaw == null) return null;
  if (!isRecord(artifactRaw)) {
    throw new Error("Theme registry validation error: artifact must be an object or null");
  }

  const size = artifactRaw.size;
  if (typeof size !== "number" || !Number.isFinite(size) || size <= 0) {
    throw new Error("Theme registry validation error: artifact.size must be a positive number");
  }

  return {
    url: readString(artifactRaw, "url"),
    sha256: readString(artifactRaw, "sha256").toLowerCase(),
    size,
    published_at: readString(artifactRaw, "published_at"),
  };
}

function validateThemeRegistryEntry(input: unknown): ThemeRegistryEntry {
  if (!isRecord(input)) {
    throw new Error("Theme registry validation error: theme entry must be an object");
  }

  const theme = validateThemeDefinition(input.theme);

  return {
    kind: "theme",
    id: readString(input, "id"),
    name: readString(input, "name"),
    version: readString(input, "version"),
    summary: readString(input, "summary"),
    description: readString(input, "description"),
    author: readString(input, "author"),
    license: readString(input, "license"),
    repository: readOptionalString(input, "repository"),
    categories: readStringArray(input, "categories"),
    tags: readStringArray(input, "tags"),
    styles: readStringArray(input, "styles"),
    icon: readOptionalString(input, "icon"),
    screenshots: readStringArray(input, "screenshots"),
    artifact: readArtifact(input),
    theme,
  };
}

function validateThemeRegistry(
  frontmatter: Record<string, unknown>,
): ThemeRegistry {
  const schemaVersion = frontmatter.schema_version;
  if (schemaVersion !== 1) {
    throw new Error(
      `Unsupported theme registry schema version: ${String(schemaVersion)} (expected 1)`,
    );
  }

  const generatedAt = readString(frontmatter, "generated_at");
  const themesRaw = frontmatter.themes;
  if (!Array.isArray(themesRaw)) {
    throw new Error("Theme registry validation error: 'themes' must be an array");
  }

  return {
    schema_version: 1,
    generated_at: generatedAt,
    themes: themesRaw.map((theme) => validateThemeRegistryEntry(theme)),
  };
}

export async function fetchThemeRegistry(
  registryUrl: string = DEFAULT_THEME_REGISTRY_URL,
): Promise<ThemeRegistry> {
  if (cachedRegistry) return cachedRegistry;

  if (
    !TRUSTED_THEME_REGISTRY_URLS.includes(
      registryUrl as (typeof TRUSTED_THEME_REGISTRY_URLS)[number],
    )
  ) {
    throw new Error(`Untrusted theme registry URL: ${registryUrl}`);
  }

  const resp = await fetch(registryUrl);
  if (!resp.ok) {
    throw new Error(`Theme registry fetch failed: ${resp.status}`);
  }

  const text = await resp.text();
  const { frontmatter } = parseMarkdownFrontmatter(text);
  cachedRegistry = validateThemeRegistry(frontmatter);
  return cachedRegistry;
}

export function clearThemeRegistryCache(): void {
  cachedRegistry = null;
}

export function getTrustedThemeRegistryUrls(): readonly string[] {
  return TRUSTED_THEME_REGISTRY_URLS;
}
