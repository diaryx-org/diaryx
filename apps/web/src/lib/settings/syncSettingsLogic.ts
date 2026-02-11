import type { UserStorageUsageResponse } from "$lib/auth";

export type StorageUsageState = "none" | "ok" | "warning" | "over_limit";

export function getStorageUsageState(
  usage: UserStorageUsageResponse | null,
): StorageUsageState {
  if (!usage) return "none";
  if (usage.over_limit) return "over_limit";
  if (
    usage.limit_bytes !== null &&
    usage.limit_bytes > 0 &&
    usage.used_bytes / usage.limit_bytes >= usage.warning_threshold
  ) {
    return "warning";
  }
  return "ok";
}

export function getUsageSummary(
  usage: UserStorageUsageResponse | null,
  formatBytes: (bytes: number) => string,
): string | null {
  if (!usage || usage.limit_bytes === null) return null;
  return `${formatBytes(usage.used_bytes)} / ${formatBytes(usage.limit_bytes)}`;
}
