import yaml from "js-yaml";

import type {
  ContentWidth,
  FontFamily,
  TypographyDefinition,
} from "$lib/stores/appearance.types";
import type { TypographyRegistryEntry } from "./types";

export interface TypographyRegistry {
  schema_version: 1;
  generated_at: string;
  typographies: TypographyRegistryEntry[];
}

const TRUSTED_TYPOGRAPHY_REGISTRY_URLS = [
  "https://cdn.diaryx.org/typographies/registry.md",
] as const;

const DEFAULT_TYPOGRAPHY_REGISTRY_URL = TRUSTED_TYPOGRAPHY_REGISTRY_URLS[0];

let cachedRegistry: TypographyRegistry | null = null;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function readString(obj: Record<string, unknown>, key: string): string {
  const value = obj[key];
  if (typeof value !== "string" || value.length === 0) {
    throw new Error(
      `Typography registry validation error: '${key}' must be a non-empty string`,
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
      `Typography registry validation error: '${key}' must be a string or null`,
    );
  }
  return value;
}

function readStringArray(obj: Record<string, unknown>, key: string): string[] {
  const value = obj[key];
  if (!Array.isArray(value)) return [];
  if (value.some((v) => typeof v !== "string")) {
    throw new Error(
      `Typography registry validation error: '${key}' must be a string[]`,
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
    throw new Error("Typography registry validation error: missing YAML frontmatter");
  }

  const frontmatter = yaml.load(match[1]) as Record<string, unknown>;
  if (!isRecord(frontmatter)) {
    throw new Error(
      "Typography registry validation error: frontmatter must be a YAML mapping",
    );
  }

  return { frontmatter, body: match[2] };
}

function isFontFamily(value: unknown): value is FontFamily {
  return (
    value === "inter" ||
    value === "system" ||
    value === "serif" ||
    value === "mono"
  );
}

function isContentWidth(value: unknown): value is ContentWidth {
  return (
    value === "narrow" ||
    value === "medium" ||
    value === "wide" ||
    value === "full"
  );
}

function validateTypographyDefinition(input: unknown): TypographyDefinition {
  if (!isRecord(input)) {
    throw new Error("Typography registry validation error: typography must be an object");
  }

  const version = input.version;
  if (version !== 1) {
    throw new Error(
      `Typography registry validation error: typography.version must be 1 (received ${String(version)})`,
    );
  }

  const settings = input.settings;
  if (!isRecord(settings)) {
    throw new Error(
      "Typography registry validation error: typography.settings must be an object",
    );
  }

  if (!isFontFamily(settings.fontFamily)) {
    throw new Error(
      "Typography registry validation error: typography.settings.fontFamily is invalid",
    );
  }

  if (
    typeof settings.baseFontSize !== "number" ||
    !Number.isFinite(settings.baseFontSize)
  ) {
    throw new Error(
      "Typography registry validation error: typography.settings.baseFontSize must be a number",
    );
  }

  if (
    typeof settings.lineHeight !== "number" ||
    !Number.isFinite(settings.lineHeight)
  ) {
    throw new Error(
      "Typography registry validation error: typography.settings.lineHeight must be a number",
    );
  }

  if (!isContentWidth(settings.contentWidth)) {
    throw new Error(
      "Typography registry validation error: typography.settings.contentWidth is invalid",
    );
  }

  return {
    id: readString(input, "id"),
    name: readString(input, "name"),
    description: readOptionalString(input, "description") ?? undefined,
    author: readOptionalString(input, "author") ?? undefined,
    version: 1,
    settings: {
      fontFamily: settings.fontFamily,
      baseFontSize: settings.baseFontSize,
      lineHeight: settings.lineHeight,
      contentWidth: settings.contentWidth,
    },
  };
}

function readArtifact(
  input: Record<string, unknown>,
): TypographyRegistryEntry["artifact"] {
  const artifactRaw = input.artifact;
  if (artifactRaw == null) return null;
  if (!isRecord(artifactRaw)) {
    throw new Error("Typography registry validation error: artifact must be an object or null");
  }

  const size = artifactRaw.size;
  if (typeof size !== "number" || !Number.isFinite(size) || size <= 0) {
    throw new Error("Typography registry validation error: artifact.size must be a positive number");
  }

  return {
    url: readString(artifactRaw, "url"),
    sha256: readString(artifactRaw, "sha256").toLowerCase(),
    size,
    published_at: readString(artifactRaw, "published_at"),
  };
}

function validateTypographyRegistryEntry(input: unknown): TypographyRegistryEntry {
  if (!isRecord(input)) {
    throw new Error("Typography registry validation error: typography entry must be an object");
  }

  const typography = validateTypographyDefinition(input.typography);

  return {
    kind: "typography",
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
    typography,
  };
}

function validateTypographyRegistry(
  frontmatter: Record<string, unknown>,
): TypographyRegistry {
  const schemaVersion = frontmatter.schema_version;
  if (schemaVersion !== 1) {
    throw new Error(
      `Unsupported typography registry schema version: ${String(schemaVersion)} (expected 1)`,
    );
  }

  const generatedAt = readString(frontmatter, "generated_at");
  const typographiesRaw = frontmatter.typographies;
  if (!Array.isArray(typographiesRaw)) {
    throw new Error("Typography registry validation error: 'typographies' must be an array");
  }

  return {
    schema_version: 1,
    generated_at: generatedAt,
    typographies: typographiesRaw.map((entry) => validateTypographyRegistryEntry(entry)),
  };
}

export async function fetchTypographyRegistry(
  registryUrl: string = DEFAULT_TYPOGRAPHY_REGISTRY_URL,
): Promise<TypographyRegistry> {
  if (cachedRegistry) return cachedRegistry;

  if (
    !TRUSTED_TYPOGRAPHY_REGISTRY_URLS.includes(
      registryUrl as (typeof TRUSTED_TYPOGRAPHY_REGISTRY_URLS)[number],
    )
  ) {
    throw new Error(`Untrusted typography registry URL: ${registryUrl}`);
  }

  const resp = await fetch(registryUrl);
  if (!resp.ok) {
    throw new Error(`Typography registry fetch failed: ${resp.status}`);
  }

  const text = await resp.text();
  const { frontmatter } = parseMarkdownFrontmatter(text);
  cachedRegistry = validateTypographyRegistry(frontmatter);
  return cachedRegistry;
}

export function clearTypographyRegistryCache(): void {
  cachedRegistry = null;
}

export function getTrustedTypographyRegistryUrls(): readonly string[] {
  return TRUSTED_TYPOGRAPHY_REGISTRY_URLS;
}
