/**
 * Human-friendly translation of publish/preview errors.
 *
 * Publishing walks the workspace, reads files, and talks to the namespace
 * server — so failures surface as low-level messages ("Failed to read file
 * 'X': No such file or directory (os error 2)", HTTP 401, sandbox permission
 * errors, …). Those are accurate but opaque in the publish dialog. This maps
 * the common cases to a short, actionable title while keeping the raw text as
 * `detail` for the console and an expandable line in the UI.
 */

export interface FriendlyPublishError {
  /** Short, user-facing summary (the toast/alert headline). */
  title: string;
  /** Longer explanation + the raw error, for the console and alert subtext. */
  detail?: string;
  /** The original error message, unmodified. */
  raw: string;
}

function rawMessage(err: unknown): string {
  if (err instanceof Error) return err.message;
  if (typeof err === 'string') return err;
  if (err == null) return '';
  try {
    return String((err as { message?: unknown }).message ?? err);
  } catch {
    return String(err);
  }
}

/**
 * Map a publish/preview error to a friendly title + detail. Always returns the
 * raw message so callers can log it or show a "details" line.
 */
export function describePublishError(
  err: unknown,
  fallback = 'Publishing failed',
): FriendlyPublishError {
  const raw = rawMessage(err).trim();
  const lower = raw.toLowerCase();

  const notFound =
    lower.includes('no such file') ||
    lower.includes('os error 2') ||
    lower.includes('enoent') ||
    lower.includes('cannot find') ||
    lower.includes('not found');

  // A workspace file couldn't be read while collecting sources — almost always
  // a moved/deleted file or a broken `contents` / `part_of` link.
  // The path itself may contain an apostrophe (e.g. 'Adam's Archive.md'), so
  // don't stop at the first inner quote — anchor on the closing `':` that the
  // Rust `Failed to read file '{path}': {source}` format always emits.
  const fileMatch = raw.match(/(?:failed to read file|read file)\s*['"](.+?)['"]\s*:/i);
  if (fileMatch && notFound) {
    const file = fileMatch[1];
    return {
      title: `Couldn't read "${file}"`,
      detail:
        `A file in your workspace is missing or can't be read, so publishing was stopped. ` +
        `It may have been moved or deleted, or a link (a "contents" or "part_of" entry) ` +
        `points to a file that no longer exists. Make sure "${file}" exists in your ` +
        `workspace, then try again.\n\n${raw}`,
      raw,
    };
  }

  // Couldn't locate the workspace's root index file.
  if (
    lower.includes('workspace root index') ||
    lower.includes('no active workspace') ||
    lower.includes('could not find the workspace root')
  ) {
    return {
      title: 'Workspace root index not found',
      detail:
        `Diaryx couldn't find this workspace's root index — the file that has a "contents" ` +
        `list and no "part_of". Reopen the workspace folder, then try again.\n\n${raw}`,
      raw,
    };
  }

  // Lost filesystem access (macOS security-scoped bookmark / sandbox).
  if (
    lower.includes('permission denied') ||
    lower.includes('operation not permitted') ||
    lower.includes('os error 1') ||
    lower.includes('os error 13') ||
    lower.includes('not authorized to access')
  ) {
    return {
      title: 'Lost access to your workspace folder',
      detail:
        `Diaryx no longer has permission to read your workspace files. Reopen the ` +
        `workspace folder to restore access, then try again.\n\n${raw}`,
      raw,
    };
  }

  // Auth / session.
  if (
    lower.includes('unauthorized') ||
    lower.includes(' 401') ||
    lower.includes('forbidden') ||
    lower.includes(' 403') ||
    lower.includes('expired token') ||
    lower.includes('invalid token') ||
    lower.includes('not signed in')
  ) {
    return {
      title: 'Your session expired — sign in again',
      detail: raw,
      raw,
    };
  }

  // Network / server reachability.
  if (
    lower.includes('failed to fetch') ||
    lower.includes('networkerror') ||
    lower.includes('network error') ||
    lower.includes('timed out') ||
    lower.includes('timeout') ||
    lower.includes('connection refused') ||
    lower.includes('econnrefused') ||
    lower.includes('dns')
  ) {
    return {
      title: "Couldn't reach the publishing server",
      detail:
        `Publishing needs a network connection to your server. Check your connection ` +
        `and try again.\n\n${raw}`,
      raw,
    };
  }

  // Server-side build failure.
  if (lower.includes('server-side render') || lower.includes('build) failed') || lower.includes('build failed')) {
    return {
      title: 'The server failed to build your site',
      detail:
        `Your files were uploaded, but the server couldn't render the site. This is ` +
        `usually temporary — try publishing again in a moment.\n\n${raw}`,
      raw,
    };
  }

  return {
    title: raw || fallback,
    detail: raw && raw !== fallback ? raw : undefined,
    raw: raw || fallback,
  };
}
