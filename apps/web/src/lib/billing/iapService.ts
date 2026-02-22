/**
 * Apple IAP Service — wraps tauri-plugin-iap for StoreKit 2.
 *
 * Provides product fetching, purchase initiation, restore, and status checking.
 */

import { invoke } from "@tauri-apps/api/core";

const PRODUCT_ID = "diaryx_plus_monthly";

export interface IapProduct {
  id: string;
  title: string;
  description: string;
  price: string;
  priceLocale: string;
}

export interface IapPurchaseResult {
  transactionId: string;
  originalTransactionId: string;
  productId: string;
  signedTransaction: string;
}

export type PurchaseState = "purchased" | "pending" | "failed" | "cancelled";

/**
 * Fetch product details from the App Store.
 * Returns localized price information for display.
 */
export async function fetchProducts(): Promise<IapProduct[]> {
  return invoke<IapProduct[]>("plugin:iap|get_products", {
    productIds: [PRODUCT_ID],
  });
}

/**
 * Initiate a purchase of Diaryx Plus.
 *
 * @param userId - Diaryx user UUID, passed as appAccountToken for server-side linking.
 * @returns The signed transaction JWS string on success.
 */
export async function purchasePlus(
  userId: string,
): Promise<IapPurchaseResult | null> {
  try {
    const result = await invoke<IapPurchaseResult>("plugin:iap|purchase", {
      productId: PRODUCT_ID,
      appAccountToken: userId,
    });
    return result;
  } catch (e: unknown) {
    const msg = e instanceof Error ? e.message : String(e);
    if (msg.includes("cancelled") || msg.includes("userCancelled")) {
      return null;
    }
    throw e;
  }
}

/**
 * Restore previous purchases (required by App Store Review Guidelines).
 * Returns all signed transactions for active subscriptions.
 */
export async function restoreIapPurchases(): Promise<string[]> {
  return invoke<string[]>("plugin:iap|restore_purchases");
}

/**
 * Check current subscription entitlements locally via StoreKit.
 */
export async function checkSubscriptionStatus(): Promise<boolean> {
  try {
    const result = await invoke<{ isSubscribed: boolean }>(
      "plugin:iap|get_subscription_status",
      { productId: PRODUCT_ID },
    );
    return result.isSubscribed;
  } catch {
    return false;
  }
}

/**
 * Get the product ID for Diaryx Plus.
 */
export function getPlusProductId(): string {
  return PRODUCT_ID;
}
