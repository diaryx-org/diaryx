/**
 * Deep Link Controller
 *
 * Routes incoming deep links / universal links into app actions. This handles
 * both:
 *
 *   - Universal Links (iOS / macOS / Android):
 *       https://app.diaryx.org/open?path=2026/06/14.md
 *   - The `diaryx://` custom scheme (desktop fallback on Windows/Linux):
 *       diaryx://open?path=2026/06/14.md
 *
 * Supported actions:
 *   - open   ?path=<workspace-relative or absolute path>   → open an entry
 *   - new    [?title=<title>][&parent=<parent path>]        → create an entry
 *   - search [?q=<query>]                                   → open command palette
 *
 * The plugin (`@tauri-apps/plugin-deep-link`) is imported dynamically so this
 * module is safe to reference from non-Tauri (web) builds. Callers should still
 * guard with `isTauri()` before invoking `registerDeepLinks`.
 */

export interface DeepLinkHandlers {
  /** Open an existing entry by path (workspace-relative or absolute). */
  openEntry: (path: string) => void | Promise<void>;
  /** Create a new entry, optionally with a title and parent entry path. */
  newEntry: (options: { title?: string; parent?: string }) => void | Promise<void>;
  /** Surface search / the command palette, optionally seeded with a query. */
  search: (query?: string) => void | Promise<void>;
}

export interface ParsedDeepLink {
  action: string;
  params: URLSearchParams;
}

/**
 * Parse a deep link URL into an action + query params.
 *
 * For the custom scheme the action lives in the host segment
 * (`diaryx://open?...` → host `open`); for https links it is the first path
 * segment (`https://app.diaryx.org/open?...` → `open`). Returns `null` for
 * URLs we can't parse.
 */
export function parseDeepLink(raw: string): ParsedDeepLink | null {
  let url: URL;
  try {
    url = new URL(raw);
  } catch {
    console.warn("[DeepLink] Ignoring unparseable URL:", raw);
    return null;
  }

  const action =
    url.protocol === "diaryx:"
      ? (url.hostname || url.pathname.replace(/^\/+/, "").split("/")[0])
      : url.pathname.replace(/^\/+/, "").split("/")[0];

  if (!action) return null;

  return { action: action.toLowerCase(), params: url.searchParams };
}

/** Dispatch a single parsed deep link to the appropriate handler. */
async function dispatch(raw: string, handlers: DeepLinkHandlers): Promise<void> {
  const parsed = parseDeepLink(raw);
  if (!parsed) return;

  const { action, params } = parsed;
  try {
    switch (action) {
      case "open": {
        const path = params.get("path");
        if (path) {
          await handlers.openEntry(path);
        } else {
          console.warn("[DeepLink] `open` is missing required `path` param");
        }
        break;
      }
      case "new": {
        await handlers.newEntry({
          title: params.get("title") ?? undefined,
          parent: params.get("parent") ?? undefined,
        });
        break;
      }
      case "search": {
        await handlers.search(params.get("q") ?? undefined);
        break;
      }
      default:
        console.warn(`[DeepLink] Unknown action: ${action}`);
    }
  } catch (err) {
    console.error(`[DeepLink] Failed to handle action "${action}":`, err);
  }
}

/**
 * Register deep link handling. Processes the launch URL (cold start) and
 * subscribes to URLs received while the app is running (warm start).
 *
 * Returns an unlisten function, or `null` if the plugin is unavailable.
 */
export async function registerDeepLinks(
  handlers: DeepLinkHandlers,
): Promise<(() => void) | null> {
  let plugin: typeof import("@tauri-apps/plugin-deep-link");
  try {
    plugin = await import("@tauri-apps/plugin-deep-link");
  } catch (err) {
    console.warn("[DeepLink] plugin unavailable, deep links disabled:", err);
    return null;
  }

  // Cold start: the app may have been launched by a deep link.
  try {
    const initial = await plugin.getCurrent();
    if (initial) {
      for (const url of initial) {
        await dispatch(url, handlers);
      }
    }
  } catch (err) {
    console.warn("[DeepLink] Failed to read launch URL:", err);
  }

  // Warm start: links received while the app is already open.
  try {
    return await plugin.onOpenUrl((urls) => {
      for (const url of urls) {
        void dispatch(url, handlers);
      }
    });
  } catch (err) {
    console.warn("[DeepLink] Failed to subscribe to open-url events:", err);
    return null;
  }
}
