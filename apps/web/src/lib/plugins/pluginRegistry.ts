/**
 * Plugin Registry client.
 *
 * Fetches a curated, versioned marketplace catalog from a trusted `registry.md`
 * URL. The registry is a markdown file with YAML frontmatter containing the
 * plugin array.
 */

import yaml from "js-yaml";
import { proxyFetch } from "$lib/backend/proxyFetch";

export interface PluginArtifact {
  url: string;
  sha256: string;
  size: number;
  published_at: string | null;
}

export interface RegistryUiEntry {
  slot: string;
  id: string;
  label: string;
  description?: string | null;
}

export interface MarketplaceEntry {
  id: string;
  name: string;
  version: string;
  summary: string;
  description: string;
  author: string;
  license: string;
  repository: string | null;
  categories: string[];
  tags: string[];
  artifact: PluginArtifact;
  capabilities: string[];
  icon: string | null;
  screenshots: string[];
  requested_permissions: unknown | null;
  ui?: RegistryUiEntry[];
}

export interface MarketplaceRegistry {
  schema_version: 2;
  generated_at: string;
  plugins: MarketplaceEntry[];
}

// Backward-compatible aliases for downstream consumers.
export type RegistryPlugin = MarketplaceEntry;
export type RegistryPluginArtifact = PluginArtifact;
export type PluginRegistryV2 = MarketplaceRegistry;

import { CDN_BASE_URL } from "$lib/marketplace/cdnBase";

const TRUSTED_REGISTRY_URLS = [
  `${CDN_BASE_URL}/plugins/registry.md`,
] as const;

const DEFAULT_REGISTRY_URL = TRUSTED_REGISTRY_URLS[0];

let cachedRegistry: MarketplaceRegistry | null = null;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function readString(obj: Record<string, unknown>, key: string): string {
  const value = obj[key];
  if (typeof value !== "string" || value.length === 0) {
    throw new Error(`Plugin registry validation error: '${key}' must be a non-empty string`);
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
    throw new Error(`Plugin registry validation error: '${key}' must be a string or null`);
  }
  return value;
}

function readPublishedAt(value: unknown): string | null {
  if (typeof value === "string" && value.length > 0) return value;
  if (value instanceof Date && !isNaN(value.getTime())) return value.toISOString();
  return null;
}

function readStringArray(obj: Record<string, unknown>, key: string): string[] {
  const value = obj[key];
  if (!Array.isArray(value)) return [];
  if (value.some((v) => typeof v !== "string")) {
    throw new Error(`Plugin registry validation error: '${key}' must be a string[]`);
  }
  return value;
}

function parseMarkdownFrontmatter(text: string): { frontmatter: Record<string, unknown>; body: string } {
  const match = text.match(/^---\r?\n([\s\S]*?)\r?\n---\r?\n([\s\S]*)$/);
  if (!match) {
    throw new Error("Plugin registry validation error: missing YAML frontmatter");
  }
  const frontmatter = yaml.load(match[1]) as Record<string, unknown>;
  if (!isRecord(frontmatter)) {
    throw new Error("Plugin registry validation error: frontmatter must be a YAML mapping");
  }
  return { frontmatter, body: match[2] };
}

function validateMarketplaceEntry(input: unknown): MarketplaceEntry {
  if (!isRecord(input)) {
    throw new Error("Plugin registry validation error: plugin entry must be an object");
  }

  const artifactRaw = input.artifact;
  if (!isRecord(artifactRaw)) {
    throw new Error("Plugin registry validation error: plugin.artifact must be an object");
  }

  const size = artifactRaw.size;
  if (typeof size !== "number" || !Number.isFinite(size) || size <= 0) {
    throw new Error("Plugin registry validation error: plugin.artifact.size must be a positive number");
  }

  return {
    id: readString(input, "id"),
    name: readString(input, "name"),
    version: readString(input, "version"),
    summary: readString(input, "summary"),
    description: readString(input, "description"),
    author: readString(input, "author"),
    license: readString(input, "license"),
    artifact: {
      url: readString(artifactRaw, "url"),
      sha256: readString(artifactRaw, "sha256").toLowerCase(),
      size,
      published_at: readPublishedAt(artifactRaw.published_at),
    },
    repository: readOptionalString(input, "repository"),
    categories: readStringArray(input, "categories"),
    tags: readStringArray(input, "tags"),
    icon: readOptionalString(input, "icon"),
    screenshots: readStringArray(input, "screenshots"),
    capabilities: readStringArray(input, "capabilities"),
    requested_permissions: input.requested_permissions ?? null,
    ui: Array.isArray(input.ui)
      ? (input.ui as Record<string, unknown>[]).filter(
          (e) =>
            typeof e.slot === "string" &&
            typeof e.id === "string" &&
            typeof e.label === "string",
        ) as unknown as RegistryUiEntry[]
      : undefined,
  };
}

function validateMarketplaceRegistry(frontmatter: Record<string, unknown>): MarketplaceRegistry {
  const schemaVersion = frontmatter.schema_version;
  if (schemaVersion !== 2) {
    throw new Error(
      `Unsupported plugin registry schema version: ${String(schemaVersion)} (expected 2)`,
    );
  }

  const generatedAt = readString(frontmatter, "generated_at");
  const pluginsRaw = frontmatter.plugins;
  if (!Array.isArray(pluginsRaw)) {
    throw new Error("Plugin registry validation error: 'plugins' must be an array");
  }

  const plugins: MarketplaceEntry[] = [];
  for (const raw of pluginsRaw) {
    try {
      plugins.push(validateMarketplaceEntry(raw));
    } catch (err) {
      const id = isRecord(raw) && typeof raw.id === "string" ? raw.id : "<unknown>";
      console.warn(`Skipping invalid plugin registry entry '${id}':`, err);
    }
  }

  return {
    schema_version: 2,
    generated_at: generatedAt,
    plugins,
  };
}

export async function fetchPluginRegistry(
  registryUrl: string = DEFAULT_REGISTRY_URL,
): Promise<MarketplaceRegistry> {
  if (cachedRegistry) return cachedRegistry;

  if (!TRUSTED_REGISTRY_URLS.includes(registryUrl as (typeof TRUSTED_REGISTRY_URLS)[number])) {
    throw new Error(`Untrusted plugin registry URL: ${registryUrl}`);
  }

  const resp = await proxyFetch(registryUrl);
  if (!resp.ok) {
    throw new Error(`Registry fetch failed: ${resp.status}`);
  }

  const text = await resp.text();
  const { frontmatter } = parseMarkdownFrontmatter(text);
  cachedRegistry = validateMarketplaceRegistry(frontmatter);
  return cachedRegistry;
}

export function clearRegistryCache(): void {
  cachedRegistry = null;
}

export function getTrustedRegistryUrls(): readonly string[] {
  return TRUSTED_REGISTRY_URLS;
}

export interface RegistryWorkspaceProvider {
  pluginId: string;
  label: string;
  description: string | null;
}

export function getRegistryWorkspaceProviders(
  plugins: MarketplaceEntry[],
  pluginIds: string[],
): RegistryWorkspaceProvider[] {
  const idSet = new Set(pluginIds);
  const result: RegistryWorkspaceProvider[] = [];
  for (const plugin of plugins) {
    if (!idSet.has(plugin.id) || !plugin.ui) continue;
    for (const ui of plugin.ui) {
      if (ui.slot === "WorkspaceProvider") {
        result.push({
          pluginId: plugin.id,
          label: ui.label,
          description: ui.description ?? null,
        });
      }
    }
  }
  return result;
}
