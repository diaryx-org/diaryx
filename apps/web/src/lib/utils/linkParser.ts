/** Regex matching a markdown link: `[Title](/path)` */
export const LINK_RE = /^\[([^\]]*)\]\(([^)]+)\)$/;

/** Parse a markdown-style link string into title and path, or null if not a link. */
export function parseLinkDisplay(
  link: string,
): { title: string; path: string } | null {
  const m = LINK_RE.exec(link);
  if (!m) return null;
  let path = m[2];
  if (path.startsWith("<") && path.endsWith(">")) {
    path = path.slice(1, -1);
  }
  return { title: m[1], path };
}
