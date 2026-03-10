import type { TemplateRegistryEntry } from "./types";

export interface TemplateArtifactPayload {
  name: string;
  content: string;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

export function parseTemplateArtifact(payload: unknown): TemplateArtifactPayload {
  if (!isRecord(payload)) {
    throw new Error("Template artifact must be an object");
  }

  const name = payload.name;
  if (typeof name !== "string" || name.length === 0) {
    throw new Error("Template artifact 'name' must be a non-empty string");
  }

  const content = payload.content;
  if (typeof content !== "string") {
    throw new Error("Template artifact 'content' must be a string");
  }

  return { name, content };
}

export interface TemplateInstallRuntime {
  saveTemplate(name: string, content: string, workspacePath: string): Promise<void>;
  listTemplateNames(): Promise<string[]>;
}

export async function fetchTemplateArtifact(
  entry: TemplateRegistryEntry,
): Promise<TemplateArtifactPayload> {
  if (!entry.artifact) {
    throw new Error(`Template '${entry.id}' has no artifact`);
  }

  const resp = await fetch(entry.artifact.url);
  if (!resp.ok) {
    throw new Error(`Failed to fetch template artifact: ${resp.status}`);
  }

  const payload = await resp.json();
  return parseTemplateArtifact(payload);
}

export async function installMarketplaceTemplate(
  entry: TemplateRegistryEntry,
  workspacePath: string,
  runtime: TemplateInstallRuntime,
): Promise<void> {
  const artifact = await fetchTemplateArtifact(entry);
  await runtime.saveTemplate(artifact.name, artifact.content, workspacePath);
}

export async function isTemplateInstalled(
  entry: TemplateRegistryEntry,
  runtime: TemplateInstallRuntime,
): Promise<boolean> {
  const names = await runtime.listTemplateNames();
  return names.some(
    (n) => n === entry.id || n === entry.name || n.toLowerCase() === entry.name.toLowerCase(),
  );
}
