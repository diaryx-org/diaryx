import type { StarterWorkspaceRegistryEntry } from "./types";

/**
 * Fetch a starter workspace ZIP archive from the CDN.
 *
 * Starter workspaces are plain diaryx workspaces packaged as ZIP files.
 * Each file already contains the correct frontmatter (part_of, contents, etc.),
 * so the caller can import them directly via `backend.importFromZip()`.
 */
export async function fetchStarterWorkspaceZip(
  entry: StarterWorkspaceRegistryEntry,
): Promise<Blob> {
  if (!entry.artifact) {
    throw new Error(`Starter workspace '${entry.id}' has no artifact`);
  }

  const resp = await fetch(entry.artifact.url);
  if (!resp.ok) {
    throw new Error(`Failed to fetch starter workspace artifact: ${resp.status}`);
  }

  return resp.blob();
}
