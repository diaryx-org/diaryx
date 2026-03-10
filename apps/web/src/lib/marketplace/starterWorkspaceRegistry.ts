import yaml from "js-yaml";

import type { StarterWorkspaceRegistryEntry } from "./types";

export interface StarterWorkspaceRegistry {
  schema_version: 1;
  generated_at: string;
  starters: StarterWorkspaceRegistryEntry[];
}

const TRUSTED_STARTER_REGISTRY_URLS = [
  "https://cdn.diaryx.org/starter-workspaces/registry.md",
] as const;

const DEFAULT_STARTER_REGISTRY_URL = TRUSTED_STARTER_REGISTRY_URLS[0];

let cachedRegistry: StarterWorkspaceRegistry | null = null;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function readString(obj: Record<string, unknown>, key: string): string {
  const value = obj[key];
  if (typeof value !== "string" || value.length === 0) {
    throw new Error(
      `Starter workspace registry validation error: '${key}' must be a non-empty string`,
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
      `Starter workspace registry validation error: '${key}' must be a string or null`,
    );
  }
  return value;
}

function readStringArray(obj: Record<string, unknown>, key: string): string[] {
  const value = obj[key];
  if (!Array.isArray(value)) return [];
  if (value.some((v) => typeof v !== "string")) {
    throw new Error(
      `Starter workspace registry validation error: '${key}' must be a string[]`,
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
    throw new Error("Starter workspace registry validation error: missing YAML frontmatter");
  }

  const frontmatter = yaml.load(match[1]) as Record<string, unknown>;
  if (!isRecord(frontmatter)) {
    throw new Error(
      "Starter workspace registry validation error: frontmatter must be a YAML mapping",
    );
  }

  return { frontmatter, body: match[2] };
}

function readArtifact(
  input: Record<string, unknown>,
): StarterWorkspaceRegistryEntry["artifact"] {
  const artifactRaw = input.artifact;
  if (artifactRaw == null) return null;
  if (!isRecord(artifactRaw)) {
    throw new Error("Starter workspace registry validation error: artifact must be an object or null");
  }

  const size = artifactRaw.size;
  if (typeof size !== "number" || !Number.isFinite(size) || size <= 0) {
    throw new Error("Starter workspace registry validation error: artifact.size must be a positive number");
  }

  return {
    url: readString(artifactRaw, "url"),
    sha256: readString(artifactRaw, "sha256").toLowerCase(),
    size,
    published_at: readString(artifactRaw, "published_at"),
  };
}

export function normalizeStarterWorkspaceRegistryEntry(input: unknown): StarterWorkspaceRegistryEntry {
  if (!isRecord(input)) {
    throw new Error("Starter workspace registry validation error: starter entry must be an object");
  }

  const fileCount = input.file_count;
  if (typeof fileCount !== "number" || !Number.isFinite(fileCount) || fileCount < 0) {
    throw new Error("Starter workspace registry validation error: 'file_count' must be a non-negative number");
  }

  const includesTemplates = input.includes_templates;
  if (typeof includesTemplates !== "boolean") {
    throw new Error("Starter workspace registry validation error: 'includes_templates' must be a boolean");
  }

  return {
    kind: "starter-workspace",
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
    file_count: fileCount,
    bundle_id: readOptionalString(input, "bundle_id"),
    includes_templates: includesTemplates,
  };
}

function validateStarterWorkspaceRegistry(
  frontmatter: Record<string, unknown>,
): StarterWorkspaceRegistry {
  const schemaVersion = frontmatter.schema_version;
  if (schemaVersion !== 1) {
    throw new Error(
      `Unsupported starter workspace registry schema version: ${String(schemaVersion)} (expected 1)`,
    );
  }

  const generatedAt = readString(frontmatter, "generated_at");
  const startersRaw = frontmatter.starters;
  if (!Array.isArray(startersRaw)) {
    throw new Error("Starter workspace registry validation error: 'starters' must be an array");
  }

  return {
    schema_version: 1,
    generated_at: generatedAt,
    starters: startersRaw.map((starter) => normalizeStarterWorkspaceRegistryEntry(starter)),
  };
}

export async function fetchStarterWorkspaceRegistry(
  registryUrl: string = DEFAULT_STARTER_REGISTRY_URL,
): Promise<StarterWorkspaceRegistry> {
  if (cachedRegistry) return cachedRegistry;

  if (
    !TRUSTED_STARTER_REGISTRY_URLS.includes(
      registryUrl as (typeof TRUSTED_STARTER_REGISTRY_URLS)[number],
    )
  ) {
    throw new Error(`Untrusted starter workspace registry URL: ${registryUrl}`);
  }

  const resp = await fetch(registryUrl);
  if (!resp.ok) {
    throw new Error(`Starter workspace registry fetch failed: ${resp.status}`);
  }

  const text = await resp.text();
  const { frontmatter } = parseMarkdownFrontmatter(text);
  cachedRegistry = validateStarterWorkspaceRegistry(frontmatter);
  return cachedRegistry;
}

export function clearStarterWorkspaceRegistryCache(): void {
  cachedRegistry = null;
}

export function getTrustedStarterWorkspaceRegistryUrls(): readonly string[] {
  return TRUSTED_STARTER_REGISTRY_URLS;
}
