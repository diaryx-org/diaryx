import yaml from "js-yaml";

import type { BundleRegistryEntry, SpotlightStep } from "./types";

export interface BundleRegistry {
  schema_version: 1;
  generated_at: string;
  bundles: BundleRegistryEntry[];
}

const TRUSTED_BUNDLE_REGISTRY_URLS = [
  "https://cdn.diaryx.org/bundles/registry.md",
] as const;

const DEFAULT_BUNDLE_REGISTRY_URL = TRUSTED_BUNDLE_REGISTRY_URLS[0];

let cachedRegistry: BundleRegistry | null = null;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function readString(obj: Record<string, unknown>, key: string): string {
  const value = obj[key];
  if (typeof value !== "string" || value.length === 0) {
    throw new Error(
      `Bundle registry validation error: '${key}' must be a non-empty string`,
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
      `Bundle registry validation error: '${key}' must be a string or null`,
    );
  }
  return value;
}

function readStringArray(obj: Record<string, unknown>, key: string): string[] {
  const value = obj[key];
  if (!Array.isArray(value)) return [];
  if (value.some((v) => typeof v !== "string")) {
    throw new Error(
      `Bundle registry validation error: '${key}' must be a string[]`,
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
    throw new Error("Bundle registry validation error: missing YAML frontmatter");
  }

  const frontmatter = yaml.load(match[1]) as Record<string, unknown>;
  if (!isRecord(frontmatter)) {
    throw new Error(
      "Bundle registry validation error: frontmatter must be a YAML mapping",
    );
  }

  return { frontmatter, body: match[2] };
}

function readArtifact(
  input: Record<string, unknown>,
): BundleRegistryEntry["artifact"] {
  const artifactRaw = input.artifact;
  if (artifactRaw == null) return null;
  if (!isRecord(artifactRaw)) {
    throw new Error("Bundle registry validation error: artifact must be an object or null");
  }

  const size = artifactRaw.size;
  if (typeof size !== "number" || !Number.isFinite(size) || size <= 0) {
    throw new Error("Bundle registry validation error: artifact.size must be a positive number");
  }

  return {
    url: readString(artifactRaw, "url"),
    sha256: readString(artifactRaw, "sha256").toLowerCase(),
    size,
    published_at: readString(artifactRaw, "published_at"),
  };
}

function normalizeTypography(
  value: unknown,
): BundleRegistryEntry["typography"] {
  if (value == null) return null;
  if (!isRecord(value)) {
    throw new Error("Bundle registry validation error: typography must be an object or null");
  }

  const out: NonNullable<BundleRegistryEntry["typography"]> = {};
  if (value.fontFamily != null) {
    if (
      value.fontFamily !== "inter" &&
      value.fontFamily !== "system" &&
      value.fontFamily !== "serif" &&
      value.fontFamily !== "mono"
    ) {
      throw new Error("Bundle registry validation error: typography.fontFamily is invalid");
    }
    out.fontFamily = value.fontFamily;
  }

  if (value.baseFontSize != null) {
    if (typeof value.baseFontSize !== "number" || !Number.isFinite(value.baseFontSize)) {
      throw new Error("Bundle registry validation error: typography.baseFontSize must be a number");
    }
    out.baseFontSize = value.baseFontSize;
  }

  if (value.lineHeight != null) {
    if (typeof value.lineHeight !== "number" || !Number.isFinite(value.lineHeight)) {
      throw new Error("Bundle registry validation error: typography.lineHeight must be a number");
    }
    out.lineHeight = value.lineHeight;
  }

  if (value.contentWidth != null) {
    if (
      value.contentWidth !== "narrow" &&
      value.contentWidth !== "medium" &&
      value.contentWidth !== "wide" &&
      value.contentWidth !== "full"
    ) {
      throw new Error("Bundle registry validation error: typography.contentWidth is invalid");
    }
    out.contentWidth = value.contentWidth;
  }

  return out;
}

function normalizePluginDependencies(
  value: unknown,
): BundleRegistryEntry["plugins"] {
  if (value == null) return [];
  if (!Array.isArray(value)) {
    throw new Error("Bundle registry validation error: plugins must be an array");
  }

  return value.map((item) => {
    if (!isRecord(item)) {
      throw new Error("Bundle registry validation error: plugin dependency must be an object");
    }

    const requiredRaw = item.required;
    const enableRaw = item.enable;

    if (requiredRaw != null && typeof requiredRaw !== "boolean") {
      throw new Error("Bundle registry validation error: plugin.required must be a boolean");
    }

    if (enableRaw != null && typeof enableRaw !== "boolean") {
      throw new Error("Bundle registry validation error: plugin.enable must be a boolean");
    }

    return {
      plugin_id: readString(item, "plugin_id"),
      required: requiredRaw ?? true,
      enable: enableRaw ?? true,
    };
  });
}

const VALID_SPOTLIGHT_PLACEMENTS = new Set(["top", "bottom", "left", "right"]);

function normalizeSpotlight(value: unknown): SpotlightStep[] | null {
  if (value == null) return null;
  if (!Array.isArray(value)) {
    throw new Error("Bundle registry validation error: spotlight must be an array or null");
  }

  return value.map((item) => {
    if (!isRecord(item)) {
      throw new Error("Bundle registry validation error: spotlight step must be an object");
    }

    const placement = readString(item, "placement");
    if (!VALID_SPOTLIGHT_PLACEMENTS.has(placement)) {
      throw new Error(
        `Bundle registry validation error: spotlight.placement must be one of top, bottom, left, right`,
      );
    }

    return {
      target: readString(item, "target"),
      title: readString(item, "title"),
      description: readString(item, "description"),
      placement: placement as SpotlightStep["placement"],
    };
  });
}

export function normalizeBundleRegistryEntry(input: unknown): BundleRegistryEntry {
  if (!isRecord(input)) {
    throw new Error("Bundle registry validation error: bundle entry must be an object");
  }

  return {
    kind: "bundle",
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
    icon: readOptionalString(input, "icon"),
    screenshots: readStringArray(input, "screenshots"),
    artifact: readArtifact(input),
    theme_id: readString(input, "theme_id"),
    typography_id: readOptionalString(input, "typography_id"),
    typography: normalizeTypography(input.typography),
    plugins: normalizePluginDependencies(input.plugins),
    starter_workspace_id: readOptionalString(input, "starter_workspace_id"),
    spotlight: normalizeSpotlight(input.spotlight),
  };
}

function validateBundleRegistry(
  frontmatter: Record<string, unknown>,
): BundleRegistry {
  const schemaVersion = frontmatter.schema_version;
  if (schemaVersion !== 1) {
    throw new Error(
      `Unsupported bundle registry schema version: ${String(schemaVersion)} (expected 1)`,
    );
  }

  const generatedAt = readString(frontmatter, "generated_at");
  const bundlesRaw = frontmatter.bundles;
  if (!Array.isArray(bundlesRaw)) {
    throw new Error("Bundle registry validation error: 'bundles' must be an array");
  }

  return {
    schema_version: 1,
    generated_at: generatedAt,
    bundles: bundlesRaw.map((bundle) => normalizeBundleRegistryEntry(bundle)),
  };
}

export async function fetchBundleRegistry(
  registryUrl: string = DEFAULT_BUNDLE_REGISTRY_URL,
): Promise<BundleRegistry> {
  if (cachedRegistry) return cachedRegistry;

  if (
    !TRUSTED_BUNDLE_REGISTRY_URLS.includes(
      registryUrl as (typeof TRUSTED_BUNDLE_REGISTRY_URLS)[number],
    )
  ) {
    throw new Error(`Untrusted bundle registry URL: ${registryUrl}`);
  }

  const resp = await fetch(registryUrl);
  if (!resp.ok) {
    throw new Error(`Bundle registry fetch failed: ${resp.status}`);
  }

  const text = await resp.text();
  const { frontmatter } = parseMarkdownFrontmatter(text);
  cachedRegistry = validateBundleRegistry(frontmatter);
  return cachedRegistry;
}

export function clearBundleRegistryCache(): void {
  cachedRegistry = null;
}

export function getTrustedBundleRegistryUrls(): readonly string[] {
  return TRUSTED_BUNDLE_REGISTRY_URLS;
}
