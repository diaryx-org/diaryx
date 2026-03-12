/**
 * Sync history tests: version history retrieval, file history, version
 * diffing, and version restore.
 */

import {
  test,
  expect,
  ensureSyncServer,
  ensureSyncPluginBase64,
  restartSyncServer,
  stopSpawnedSyncServer,
  setupSingleSyncClient,
  createEntryWithMarker,
  appendMarkerToEntry,
  setFrontmatterProperty,
  readEntryBody,
  openEntryForSync,
  executePluginCommand,
} from "./helpers";

type HistoryEntry = {
  update_id: number;
  timestamp: number;
  origin?: string;
  files_changed?: string[];
};

test.describe("Sync › History", () => {
  test.describe.configure({ mode: "serial" });

  test.beforeAll(async ({ browserName }) => {
    if (browserName !== "chromium") return;
    await ensureSyncServer();
    await ensureSyncPluginBase64();
  });

  test.beforeEach(async ({ browserName }) => {
    if (browserName !== "chromium") return;
    if (process.env.SYNC_E2E_RESTART_SERVER_PER_TEST !== "1") return;
    await restartSyncServer();
  });

  test.afterAll(async ({ browserName }) => {
    if (browserName !== "chromium") return;
    const previousCleanupSetting = process.env.SYNC_E2E_CLEANUP_SERVER;
    process.env.SYNC_E2E_CLEANUP_SERVER = "1";
    try {
      await stopSpawnedSyncServer();
    } finally {
      if (previousCleanupSetting === undefined) {
        delete process.env.SYNC_E2E_CLEANUP_SERVER;
      } else {
        process.env.SYNC_E2E_CLEANUP_SERVER = previousCleanupSetting;
      }
    }
  });

  test("records version history and allows retrieval", async ({ browser, browserName }) => {
    test.skip(browserName !== "chromium", "Sync E2E currently runs on Chromium only");
    test.setTimeout(300000);

    const client = await setupSingleSyncClient(browser, "history-get");
    const { page, rootPath } = client;

    try {
      const runId = `${Date.now()}-${Math.floor(Math.random() * 100000)}`;

      // Create an entry (generates first history event).
      const entryPath = await createEntryWithMarker(
        page,
        `history-${runId}`,
        `HISTORY_V1_${runId}`,
      );
      await openEntryForSync(page, entryPath);

      // Edit the entry to create more history.
      await appendMarkerToEntry(page, entryPath, `HISTORY_V2_${runId}`);
      await setFrontmatterProperty(page, entryPath, "description", `desc-${runId}`);

      // Give the plugin a moment to process the updates.
      await page.waitForTimeout(2000);

      // Retrieve workspace history.
      console.log("[sync-e2e:history] step: GetHistory");
      const history = await executePluginCommand(
        page,
        "diaryx.sync",
        "GetHistory",
        { doc_name: "workspace", limit: 50 },
      ) as HistoryEntry[] | null;

      expect(history).toBeTruthy();
      expect(Array.isArray(history)).toBe(true);
      expect(history!.length).toBeGreaterThan(0);

      // Each entry should have an update_id and timestamp.
      for (const entry of history!) {
        expect(entry.update_id).toBeDefined();
        expect(entry.timestamp).toBeDefined();
      }
    } finally {
      await client.context.close();
    }
  });

  test("retrieves file-specific history", async ({ browser, browserName }) => {
    test.skip(browserName !== "chromium", "Sync E2E currently runs on Chromium only");
    test.setTimeout(300000);

    const client = await setupSingleSyncClient(browser, "history-file");
    const { page } = client;

    try {
      const runId = `${Date.now()}-${Math.floor(Math.random() * 100000)}`;

      const entryPath = await createEntryWithMarker(
        page,
        `file-history-${runId}`,
        `FILE_HIST_V1_${runId}`,
      );
      await openEntryForSync(page, entryPath);

      // Make several edits so there are multiple history entries.
      await appendMarkerToEntry(page, entryPath, `FILE_HIST_V2_${runId}`);
      await setFrontmatterProperty(page, entryPath, "tags", [`tag-${runId}`]);
      await appendMarkerToEntry(page, entryPath, `FILE_HIST_V3_${runId}`);

      await page.waitForTimeout(2000);

      // GetFileHistory expects a workspace-relative path (e.g. "file.md"),
      // not the full absolute path returned by the bridge.
      const relativePath = entryPath.split("/").pop()!;

      console.log("[sync-e2e:history] step: GetFileHistory");
      const fileHistory = await executePluginCommand(
        page,
        "diaryx.sync",
        "GetFileHistory",
        { file_path: relativePath, limit: 50 },
      ) as HistoryEntry[] | null;

      expect(fileHistory).toBeTruthy();
      expect(Array.isArray(fileHistory)).toBe(true);
      // The file should appear in at least the workspace-level changes.
      expect(fileHistory!.length).toBeGreaterThan(0);
    } finally {
      await client.context.close();
    }
  });

  test("diffs two history versions", async ({ browser, browserName }) => {
    test.skip(browserName !== "chromium", "Sync E2E currently runs on Chromium only");
    test.setTimeout(300000);

    const client = await setupSingleSyncClient(browser, "history-diff");
    const { page } = client;

    try {
      const runId = `${Date.now()}-${Math.floor(Math.random() * 100000)}`;

      const entryPath = await createEntryWithMarker(
        page,
        `diff-${runId}`,
        `DIFF_V1_${runId}`,
      );
      await openEntryForSync(page, entryPath);
      await appendMarkerToEntry(page, entryPath, `DIFF_V2_${runId}`);
      await page.waitForTimeout(2000);

      // Get history to find two update IDs.
      const history = await executePluginCommand(
        page,
        "diaryx.sync",
        "GetHistory",
        { doc_name: "workspace", limit: 50 },
      ) as HistoryEntry[] | null;

      expect(history).toBeTruthy();
      expect(history!.length).toBeGreaterThanOrEqual(2);

      // History is newest-first, so pick the last two.
      const newerEntry = history![0];
      const olderEntry = history![history!.length - 1];

      console.log("[sync-e2e:history] step: GetVersionDiff");
      const diff = await executePluginCommand(
        page,
        "diaryx.sync",
        "GetVersionDiff",
        {
          doc_name: "workspace",
          from_id: olderEntry.update_id,
          to_id: newerEntry.update_id,
        },
      ) as Array<{ path?: string; change_type?: string }> | null;

      expect(diff).toBeTruthy();
      expect(Array.isArray(diff)).toBe(true);
      // There should be at least one file change between the two versions.
      expect(diff!.length).toBeGreaterThan(0);
    } finally {
      await client.context.close();
    }
  });

  test("restores a previous version", async ({ browser, browserName }) => {
    test.skip(browserName !== "chromium", "Sync E2E currently runs on Chromium only");
    test.setTimeout(300000);

    const client = await setupSingleSyncClient(browser, "history-restore");
    const { page } = client;

    try {
      const runId = `${Date.now()}-${Math.floor(Math.random() * 100000)}`;
      const v1Marker = `RESTORE_V1_${runId}`;

      // Create initial entry.
      const entryPath = await createEntryWithMarker(
        page,
        `restore-${runId}`,
        v1Marker,
      );
      await openEntryForSync(page, entryPath);
      await page.waitForTimeout(1000);

      // Snapshot the history to find V1's update_id.
      const historyBeforeEdit = await executePluginCommand(
        page,
        "diaryx.sync",
        "GetHistory",
        { doc_name: "workspace", limit: 50 },
      ) as HistoryEntry[] | null;

      expect(historyBeforeEdit).toBeTruthy();
      expect(historyBeforeEdit!.length).toBeGreaterThan(0);

      // Pick the oldest entry as our restore target.
      const restoreTarget = historyBeforeEdit![historyBeforeEdit!.length - 1];

      // Make a destructive edit.
      await appendMarkerToEntry(page, entryPath, `RESTORE_OVERWRITTEN_${runId}`);
      await page.waitForTimeout(1000);

      // Restore to the earlier version.
      console.log("[sync-e2e:history] step: RestoreVersion");
      await executePluginCommand(
        page,
        "diaryx.sync",
        "RestoreVersion",
        { doc_name: "workspace", update_id: restoreTarget.update_id },
      );

      // Give the restore a moment to apply.
      await page.waitForTimeout(2000);

      // History should now have more entries (the restore itself creates one).
      const historyAfterRestore = await executePluginCommand(
        page,
        "diaryx.sync",
        "GetHistory",
        { doc_name: "workspace", limit: 50 },
      ) as HistoryEntry[] | null;

      expect(historyAfterRestore).toBeTruthy();
      expect(historyAfterRestore!.length).toBeGreaterThan(historyBeforeEdit!.length);
    } finally {
      await client.context.close();
    }
  });
});
