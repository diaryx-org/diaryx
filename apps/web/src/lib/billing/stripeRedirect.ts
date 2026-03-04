/**
 * Stripe redirect utility — handles opening Stripe URLs in Tauri vs web.
 *
 * In web browsers, redirects the current page (standard Stripe flow).
 * In Tauri, opens the URL in the default browser via shell.open() so the
 * user stays in the app.
 */

import { isTauri } from "$lib/backend/interface";
import { refreshUserInfo, getAuthState } from "$lib/auth";

/**
 * Open a Stripe URL (checkout or portal) in the appropriate way.
 *
 * - Web: `window.location.href = url` (navigates away)
 * - Tauri: opens in the default browser via shell plugin
 */
export async function openStripeUrl(url: string): Promise<void> {
  if (isTauri()) {
    const { open } = await import("@tauri-apps/plugin-shell");
    await open(url);
  } else {
    window.location.href = url;
  }
}

/**
 * Open an external URL in a new tab/window (web) or the default browser (Tauri).
 *
 * Unlike `openStripeUrl`, this doesn't navigate the current page — it always
 * opens a new context. Use this for informational links like Terms & Privacy.
 */
export async function openExternalUrl(url: string): Promise<void> {
  if (isTauri()) {
    const { open } = await import("@tauri-apps/plugin-shell");
    await open(url);
  } else {
    window.open(url, "_blank", "noopener,noreferrer");
  }
}

/**
 * Poll `/auth/me` until the user's tier changes to "plus".
 * Returns true if upgrade detected, false if timed out.
 */
export async function pollForPlusUpgrade(
  maxAttempts = 40,
  intervalMs = 2000,
): Promise<boolean> {
  for (let i = 0; i < maxAttempts; i++) {
    await new Promise((r) => setTimeout(r, intervalMs));
    await refreshUserInfo();
    if (getAuthState().tier === "plus") return true;
  }
  return false;
}
