/**
 * Plugin Registry v2 client.
 *
 * Fetches a curated, versioned marketplace catalog from trusted registry URLs.
 */

export type RegistrySourceKind = "internal" | "external";

export interface RegistryPluginArtifact {
  wasmUrl: string;
  sha256: string;
  sizeBytes: number;
  publishedAt: string;
}

export interface RegistryPluginSource {
  kind: RegistrySourceKind;
  repositoryUrl: string;
  registryId: string;
}

export interface RegistryPlugin {
  id: string;
  name: string;
  version: string;
  summary: string;
  description: string;
  creator: string;
  license: string;
  artifact: RegistryPluginArtifact;
  source: RegistryPluginSource;
  homepage: string | null;
  documentationUrl: string | null;
  changelogUrl: string | null;
  categories: string[];
  tags: string[];
  iconUrl: string | null;
  screenshots: string[];
  capabilities: string[];
  requestedPermissions: unknown | null;
}

export interface PluginRegistryV2 {
  schemaVersion: 2;
  generatedAt: string;
  plugins: RegistryPlugin[];
}

const TRUSTED_REGISTRY_URLS = [
  "https://cdn.diaryx.org/plugins/registry-v2.json",
] as const;

const DEFAULT_REGISTRY_URL = TRUSTED_REGISTRY_URLS[0];

let cachedRegistry: PluginRegistryV2 | null = null;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function readString(obj: Record<string, unknown>, key: string): string {
  const value = obj[key];
  if (typeof value !== "string" || value.length === 0) {
    throw new Error(`Plugin registry v2 validation error: '${key}' must be a non-empty string`);
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
    throw new Error(`Plugin registry v2 validation error: '${key}' must be a string or null`);
  }
  return value;
}

function readStringArray(obj: Record<string, unknown>, key: string): string[] {
  const value = obj[key];
  if (!Array.isArray(value) || value.some((v) => typeof v !== "string")) {
    throw new Error(`Plugin registry v2 validation error: '${key}' must be a string[]`);
  }
  return value;
}

function validateRegistryPlugin(input: unknown): RegistryPlugin {
  if (!isRecord(input)) {
    throw new Error("Plugin registry v2 validation error: plugin entry must be an object");
  }

  const artifactRaw = input.artifact;
  if (!isRecord(artifactRaw)) {
    throw new Error("Plugin registry v2 validation error: plugin.artifact must be an object");
  }

  const sourceRaw = input.source;
  if (!isRecord(sourceRaw)) {
    throw new Error("Plugin registry v2 validation error: plugin.source must be an object");
  }

  const sourceKind = readString(sourceRaw, "kind");
  if (sourceKind !== "internal" && sourceKind !== "external") {
    throw new Error("Plugin registry v2 validation error: plugin.source.kind must be 'internal' or 'external'");
  }

  const sizeBytes = artifactRaw.sizeBytes;
  if (typeof sizeBytes !== "number" || !Number.isFinite(sizeBytes) || sizeBytes <= 0) {
    throw new Error("Plugin registry v2 validation error: plugin.artifact.sizeBytes must be a positive number");
  }

  return {
    id: readString(input, "id"),
    name: readString(input, "name"),
    version: readString(input, "version"),
    summary: readString(input, "summary"),
    description: readString(input, "description"),
    creator: readString(input, "creator"),
    license: readString(input, "license"),
    artifact: {
      wasmUrl: readString(artifactRaw, "wasmUrl"),
      sha256: readString(artifactRaw, "sha256").toLowerCase(),
      sizeBytes,
      publishedAt: readString(artifactRaw, "publishedAt"),
    },
    source: {
      kind: sourceKind,
      repositoryUrl: readString(sourceRaw, "repositoryUrl"),
      registryId: readString(sourceRaw, "registryId"),
    },
    homepage: readOptionalString(input, "homepage"),
    documentationUrl: readOptionalString(input, "documentationUrl"),
    changelogUrl: readOptionalString(input, "changelogUrl"),
    categories: readStringArray(input, "categories"),
    tags: readStringArray(input, "tags"),
    iconUrl: readOptionalString(input, "iconUrl"),
    screenshots: readStringArray(input, "screenshots"),
    capabilities: readStringArray(input, "capabilities"),
    requestedPermissions: input.requestedPermissions ?? null,
  };
}

function validateRegistryV2(input: unknown): PluginRegistryV2 {
  if (!isRecord(input)) {
    throw new Error("Plugin registry v2 validation error: top-level payload must be an object");
  }

  const schemaVersion = input.schemaVersion;
  if (schemaVersion !== 2) {
    throw new Error(
      `Unsupported plugin registry schema version: ${String(schemaVersion)} (expected 2)`,
    );
  }

  const generatedAt = readString(input, "generatedAt");
  const pluginsRaw = input.plugins;
  if (!Array.isArray(pluginsRaw)) {
    throw new Error("Plugin registry v2 validation error: 'plugins' must be an array");
  }

  return {
    schemaVersion: 2,
    generatedAt,
    plugins: pluginsRaw.map((plugin) => validateRegistryPlugin(plugin)),
  };
}

export async function fetchPluginRegistry(
  registryUrl: string = DEFAULT_REGISTRY_URL,
): Promise<PluginRegistryV2> {
  if (cachedRegistry) return cachedRegistry;

  if (!TRUSTED_REGISTRY_URLS.includes(registryUrl as (typeof TRUSTED_REGISTRY_URLS)[number])) {
    throw new Error(`Untrusted plugin registry URL: ${registryUrl}`);
  }

  const resp = await fetch(registryUrl);
  if (!resp.ok) {
    throw new Error(`Registry fetch failed: ${resp.status}`);
  }

  const payload = await resp.json();
  cachedRegistry = validateRegistryV2(payload);
  return cachedRegistry;
}

export function clearRegistryCache(): void {
  cachedRegistry = null;
}

export function getTrustedRegistryUrls(): readonly string[] {
  return TRUSTED_REGISTRY_URLS;
}
