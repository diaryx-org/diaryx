import { describe, expect, it } from "vitest";
import { getStorageUsageState, getUsageSummary } from "./syncSettingsLogic";

describe("syncSettingsLogic", () => {
  it("returns usage summary when limit exists", () => {
    const summary = getUsageSummary(
      {
        used_bytes: 512 * 1024 * 1024,
        blob_count: 2,
        limit_bytes: 1024 * 1024 * 1024,
        warning_threshold: 0.8,
        over_limit: false,
        scope: "attachments",
      },
      (bytes) => `${Math.round(bytes / 1024 / 1024)} MB`,
    );
    expect(summary).toBe("512 MB / 1024 MB");
  });

  it("returns warning when threshold reached", () => {
    const state = getStorageUsageState({
      used_bytes: 900,
      blob_count: 2,
      limit_bytes: 1000,
      warning_threshold: 0.8,
      over_limit: false,
      scope: "attachments",
    });
    expect(state).toBe("warning");
  });

  it("returns over_limit when flagged", () => {
    const state = getStorageUsageState({
      used_bytes: 1200,
      blob_count: 2,
      limit_bytes: 1000,
      warning_threshold: 0.8,
      over_limit: true,
      scope: "attachments",
    });
    expect(state).toBe("over_limit");
  });
});
