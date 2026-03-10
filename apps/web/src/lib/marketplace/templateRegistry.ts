import yaml from "js-yaml";

import type { TemplateRegistryEntry } from "./types";

export interface TemplateRegistry {
  schema_version: 1;
  generated_at: string;
  templates: TemplateRegistryEntry[];
}

const TRUSTED_TEMPLATE_REGISTRY_URLS = [
  "https://cdn.diaryx.org/templates/registry.md",
] as const;

const DEFAULT_TEMPLATE_REGISTRY_URL = TRUSTED_TEMPLATE_REGISTRY_URLS[0];

let cachedRegistry: TemplateRegistry | null = null;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function readString(obj: Record<string, unknown>, key: string): string {
  const value = obj[key];
  if (typeof value !== "string" || value.length === 0) {
    throw new Error(
      `Template registry validation error: '${key}' must be a non-empty string`,
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
      `Template registry validation error: '${key}' must be a string or null`,
    );
  }
  return value;
}

function readStringArray(obj: Record<string, unknown>, key: string): string[] {
  const value = obj[key];
  if (!Array.isArray(value)) return [];
  if (value.some((v) => typeof v !== "string")) {
    throw new Error(
      `Template registry validation error: '${key}' must be a string[]`,
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
    throw new Error("Template registry validation error: missing YAML frontmatter");
  }

  const frontmatter = yaml.load(match[1]) as Record<string, unknown>;
  if (!isRecord(frontmatter)) {
    throw new Error(
      "Template registry validation error: frontmatter must be a YAML mapping",
    );
  }

  return { frontmatter, body: match[2] };
}

function readArtifact(
  input: Record<string, unknown>,
): TemplateRegistryEntry["artifact"] {
  const artifactRaw = input.artifact;
  if (artifactRaw == null) return null;
  if (!isRecord(artifactRaw)) {
    throw new Error("Template registry validation error: artifact must be an object or null");
  }

  const size = artifactRaw.size;
  if (typeof size !== "number" || !Number.isFinite(size) || size <= 0) {
    throw new Error("Template registry validation error: artifact.size must be a positive number");
  }

  return {
    url: readString(artifactRaw, "url"),
    sha256: readString(artifactRaw, "sha256").toLowerCase(),
    size,
    published_at: readString(artifactRaw, "published_at"),
  };
}

export function normalizeTemplateRegistryEntry(input: unknown): TemplateRegistryEntry {
  if (!isRecord(input)) {
    throw new Error("Template registry validation error: template entry must be an object");
  }

  return {
    kind: "template",
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
    template_variables: readStringArray(input, "template_variables"),
    preview: readOptionalString(input, "preview"),
  };
}

function validateTemplateRegistry(
  frontmatter: Record<string, unknown>,
): TemplateRegistry {
  const schemaVersion = frontmatter.schema_version;
  if (schemaVersion !== 1) {
    throw new Error(
      `Unsupported template registry schema version: ${String(schemaVersion)} (expected 1)`,
    );
  }

  const generatedAt = readString(frontmatter, "generated_at");
  const templatesRaw = frontmatter.templates;
  if (!Array.isArray(templatesRaw)) {
    throw new Error("Template registry validation error: 'templates' must be an array");
  }

  return {
    schema_version: 1,
    generated_at: generatedAt,
    templates: templatesRaw.map((template) => normalizeTemplateRegistryEntry(template)),
  };
}

export async function fetchTemplateRegistry(
  registryUrl: string = DEFAULT_TEMPLATE_REGISTRY_URL,
): Promise<TemplateRegistry> {
  if (cachedRegistry) return cachedRegistry;

  if (
    !TRUSTED_TEMPLATE_REGISTRY_URLS.includes(
      registryUrl as (typeof TRUSTED_TEMPLATE_REGISTRY_URLS)[number],
    )
  ) {
    throw new Error(`Untrusted template registry URL: ${registryUrl}`);
  }

  const resp = await fetch(registryUrl);
  if (!resp.ok) {
    throw new Error(`Template registry fetch failed: ${resp.status}`);
  }

  const text = await resp.text();
  const { frontmatter } = parseMarkdownFrontmatter(text);
  cachedRegistry = validateTemplateRegistry(frontmatter);
  return cachedRegistry;
}

export function clearTemplateRegistryCache(): void {
  cachedRegistry = null;
}

export function getTrustedTemplateRegistryUrls(): readonly string[] {
  return TRUSTED_TEMPLATE_REGISTRY_URLS;
}
