/**
 * Utility to detect tier/subscription limit errors from server responses.
 */

const TIER_LIMIT_PATTERNS = [
  "workspace limit",
  "subscription",
  "tier",
  "upgrade",
  "plan does not",
  "plus required",
  "limit reached",
  "quota exceeded",
];

/**
 * Returns true if the error appears to be a tier/subscription limit error
 * (e.g. 403 from the server because the user is on the free plan).
 */
export function isTierLimitError(error: unknown): boolean {
  const message =
    error instanceof Error
      ? error.message
      : typeof error === "string"
        ? error
        : "";
  const lower = message.toLowerCase();
  return TIER_LIMIT_PATTERNS.some((p) => lower.includes(p));
}
