/**
 * Platform billing detection.
 *
 * Determines whether to use Apple IAP or Stripe based on the runtime environment.
 */

import { getBackendSync, isTauri } from "$lib/backend";
import { isIOS } from "$lib/hooks/useMobile.svelte";

export type BillingProvider = "apple_iap" | "stripe";

/**
 * Determine which billing provider to use for the current platform.
 *
 * - iOS Tauri → apple_iap (App Store requirement)
 * - Apple App Store Tauri builds → apple_iap
 * - Web browser → stripe
 * - Other Tauri (direct desktop distribution) → stripe
 */
export function getBillingProvider(): BillingProvider {
  if (!isTauri()) return "stripe";
  if (isIOS()) return "apple_iap";
  if (isAppleBuild()) return "apple_iap";
  return "stripe";
}

function isAppleBuild(): boolean {
  try {
    return getBackendSync().getAppPaths()?.is_apple_build === true;
  } catch {
    return false;
  }
}
