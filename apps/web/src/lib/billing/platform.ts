/**
 * Platform billing detection.
 *
 * Determines whether to use Apple IAP or Stripe based on the runtime environment.
 */

import { isTauri } from "$lib/backend/interface";
import { isIOS } from "$lib/hooks/useMobile.svelte";

export type BillingProvider = "apple_iap" | "stripe";

/**
 * Determine which billing provider to use for the current platform.
 *
 * - iOS Tauri → apple_iap (App Store requirement)
 * - macOS Tauri → apple_iap (assumes Mac App Store distribution)
 * - Web browser → stripe
 * - Other Tauri (Linux, Windows) → stripe
 */
export function getBillingProvider(): BillingProvider {
  if (!isTauri()) return "stripe";
  if (isIOS()) return "apple_iap";
  // macOS: assume Mac App Store distribution → apple_iap
  if (isMacOS()) return "apple_iap";
  return "stripe";
}

function isMacOS(): boolean {
  if (typeof navigator === "undefined") return false;
  return (
    navigator.platform === "MacIntel" || navigator.platform.startsWith("Mac")
  );
}
