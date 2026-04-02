export function normalizeExtismHostPath(path: string | null | undefined): string {
  const trimmed = typeof path === "string" ? path.trim() : "";
  if (!trimmed) return ".";

  let normalized = trimmed.replace(/\\/g, "/").replace(/\/+/g, "/");
  while (normalized.startsWith("./")) {
    normalized = normalized.slice(2);
  }
  normalized = normalized.replace(/\/\.(?=\/|$)/g, "");

  // Reject directory traversal attempts
  for (const component of normalized.split("/")) {
    if (component === "..") {
      throw new Error(`Path traversal not allowed: '${path}'`);
    }
  }

  return normalized.length > 0 ? normalized : ".";
}
