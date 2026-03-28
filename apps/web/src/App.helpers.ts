/**
 * Pure helper functions extracted from App.svelte for testability.
 */

/**
 * Derives a human-readable name for the pending-delete confirmation dialog.
 *
 * - Single path: returns the filename without `.md`
 * - Multiple paths: returns a count string like "3 selected entries"
 * - No paths: returns empty string
 */
export function getPendingDeleteName(paths: string[]): string {
  if (paths.length === 1) {
    return paths[0]?.split("/").pop()?.replace(".md", "") ?? "";
  }
  if (paths.length > 1) {
    return `${paths.length} selected entries`;
  }
  return "";
}

/**
 * Derives the confirmation description shown in the delete dialog.
 */
export function getPendingDeleteDescription(
  paths: string[],
  includesDescendants: boolean,
): string {
  if (paths.length === 0) return "";

  const name = getPendingDeleteName(paths);

  if (paths.length === 1) {
    return includesDescendants
      ? `Are you sure you want to delete "${name}" and its descendants? This action cannot be undone.`
      : `Are you sure you want to delete "${name}"? This action cannot be undone.`;
  }

  return includesDescendants
    ? `Are you sure you want to delete ${paths.length} selected entries and their descendants? This action cannot be undone.`
    : `Are you sure you want to delete ${paths.length} selected entries? This action cannot be undone.`;
}

/**
 * Computes the new sidebar width during a pointer-drag resize.
 *
 * For the left sidebar the width grows when the pointer moves right (positive
 * delta). For the right sidebar the width grows when the pointer moves left
 * (negative delta).
 */
export function computeResizeWidth(
  startWidth: number,
  startX: number,
  currentX: number,
  side: "left" | "right",
): number {
  const delta = side === "left" ? currentX - startX : startX - currentX;
  return startWidth + delta;
}

/**
 * Normalizes frontmatter that may arrive as a `Map` (from WASM) into a plain
 * object.
 */
export function normalizeFrontmatter(
  frontmatter: unknown,
): Record<string, unknown> {
  if (!frontmatter) return {};
  if (frontmatter instanceof Map) {
    return Object.fromEntries(frontmatter.entries());
  }
  return frontmatter as Record<string, unknown>;
}
