/**
 * Sync offline/reconnect tests: verifies that changes made while offline
 * are queued and synced when the connection is restored.
 */

import {
  test,
  expect,
  ensureSyncServer,
  ensureSyncPluginBase64,
  restartSyncServer,
  stopSpawnedSyncServer,
  setupSyncedPair,
  createEntryWithMarker,
  appendMarkerToEntry,
  setFrontmatterProperty,
  readEntryBody,
  expectFrontmatterProperty,
  openEntryForSync,
  queueBodyUpdateForSync,
  waitForSyncSession,
} from "./helpers";

test.describe("Sync › Offline / Reconnect", () => {
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

  test("queues edits made offline and syncs them on reconnect", async ({ browser, browserName }) => {
    test.skip(browserName !== "chromium", "Sync E2E currently runs on Chromium only");
    test.setTimeout(300000);

    const pair = await setupSyncedPair(browser, "offline");
    const { pageA, pageB, contextA, contextB } = pair;

    try {
      const runId = `${Date.now()}-${Math.floor(Math.random() * 100000)}`;
      const initialMarker = `ONLINE_BODY_${runId}`;
      const offlineMarkerA = `OFFLINE_A_${runId}`;
      const offlineDesc = `offline-desc-${runId}`;

      // Create entry while both clients are online.
      const entryPath = await createEntryWithMarker(
        pageA,
        `offline-${runId}`,
        initialMarker,
      );
      await openEntryForSync(pageA, entryPath);
      await queueBodyUpdateForSync(pageA, entryPath);
      await openEntryForSync(pageB, entryPath);
      await expect
        .poll(async () => await readEntryBody(pageB, entryPath), { timeout: 30000 })
        .toContain(initialMarker);

      // Take client A offline.
      console.log("[sync-e2e:offline] step: taking pageA offline");
      await contextA.setOffline(true);

      // Make edits on A while offline.
      await appendMarkerToEntry(pageA, entryPath, offlineMarkerA);
      await setFrontmatterProperty(pageA, entryPath, "description", offlineDesc);

      // Verify A has the changes locally.
      await expect
        .poll(async () => await readEntryBody(pageA, entryPath, { sync: false }), { timeout: 10000 })
        .toContain(offlineMarkerA);

      // B should NOT see the offline changes yet.
      // Wait a few seconds to confirm no sync happens.
      await pageB.waitForTimeout(3000);
      const bodyBWhileOffline = await readEntryBody(pageB, entryPath, { sync: false });
      expect(bodyBWhileOffline).not.toContain(offlineMarkerA);

      // Bring A back online.
      console.log("[sync-e2e:offline] step: bringing pageA back online");
      await contextA.setOffline(false);

      // Wait for sync to resume. The global status can remain "syncing" while
      // unrelated body docs settle, so push the local offline body first and
      // only then ask the remote client to re-open/resubscribe that entry.
      await waitForSyncSession(pageA);
      await expect
        .poll(async () => await readEntryBody(pageA, entryPath, { sync: false }), { timeout: 10000 })
        .toContain(offlineMarkerA);
      await queueBodyUpdateForSync(pageA, entryPath);
      await openEntryForSync(pageB, entryPath);

      // B should eventually see the changes that were queued offline.
      console.log("[sync-e2e:offline] step: waiting for offline edits to propagate to pageB");
      await expect
        .poll(async () => await readEntryBody(pageB, entryPath), { timeout: 45000 })
        .toContain(offlineMarkerA);
      await expectFrontmatterProperty(pageB, entryPath, "description", offlineDesc, 30000);
    } finally {
      // Ensure we restore online state before closing.
      await contextA.setOffline(false).catch(() => undefined);
      await Promise.allSettled([contextA.close(), contextB.close()]);
    }
  });

  test("both clients edit offline and merge on reconnect", async ({ browser, browserName }) => {
    test.skip(browserName !== "chromium", "Sync E2E currently runs on Chromium only");
    test.setTimeout(300000);

    const pair = await setupSyncedPair(browser, "dual-offline");
    const { pageA, pageB, contextA, contextB } = pair;

    try {
      const runId = `${Date.now()}-${Math.floor(Math.random() * 100000)}`;
      const initialMarker = `DUAL_INIT_${runId}`;
      const offlineA = `DUAL_OFFLINE_A_${runId}`;
      const offlineB = `DUAL_OFFLINE_B_${runId}`;

      // Create entry while both online.
      const entryPath = await createEntryWithMarker(
        pageA,
        `dual-offline-${runId}`,
        initialMarker,
      );
      await openEntryForSync(pageA, entryPath);
      await queueBodyUpdateForSync(pageA, entryPath);
      await openEntryForSync(pageB, entryPath);
      await expect
        .poll(async () => await readEntryBody(pageB, entryPath), { timeout: 30000 })
        .toContain(initialMarker);

      // Take both clients offline.
      console.log("[sync-e2e:dual-offline] step: taking both clients offline");
      await Promise.all([
        contextA.setOffline(true),
        contextB.setOffline(true),
      ]);

      // Each client makes independent edits.
      await appendMarkerToEntry(pageA, entryPath, offlineA);
      await appendMarkerToEntry(pageB, entryPath, offlineB);

      // Verify local state.
      await expect
        .poll(async () => await readEntryBody(pageA, entryPath), { timeout: 10000 })
        .toContain(offlineA);
      await expect
        .poll(async () => await readEntryBody(pageB, entryPath), { timeout: 10000 })
        .toContain(offlineB);

      // Bring both clients back online.
      console.log("[sync-e2e:dual-offline] step: bringing both clients back online");
      await Promise.all([
        contextA.setOffline(false),
        contextB.setOffline(false),
      ]);

      await Promise.all([
        waitForSyncSession(pageA),
        waitForSyncSession(pageB),
      ]);

      // Re-open the entry for sync on both clients to re-establish body CRDT
      // subscriptions that may have been lost during the offline period.
      await Promise.all([
        expect
          .poll(async () => await readEntryBody(pageA, entryPath, { sync: false }), { timeout: 10000 })
          .toContain(offlineA),
        expect
          .poll(async () => await readEntryBody(pageB, entryPath, { sync: false }), { timeout: 10000 })
          .toContain(offlineB),
      ]);

      // Push each client's offline body first, then re-open to re-establish
      // subscriptions against the merged server state.
      await Promise.all([
        queueBodyUpdateForSync(pageA, entryPath),
        queueBodyUpdateForSync(pageB, entryPath),
      ]);
      console.log("[sync-e2e:dual-offline] step: re-opening entry for sync after reconnect");
      await openEntryForSync(pageA, entryPath);
      await openEntryForSync(pageB, entryPath);

      // Both clients should eventually see both offline markers (CRDT merge).
      console.log("[sync-e2e:dual-offline] step: waiting for merged content");
      await expect
        .poll(async () => await readEntryBody(pageA, entryPath), { timeout: 45000 })
        .toContain(offlineA);
      await expect
        .poll(async () => await readEntryBody(pageA, entryPath), { timeout: 30000 })
        .toContain(offlineB);

      await expect
        .poll(async () => await readEntryBody(pageB, entryPath), { timeout: 45000 })
        .toContain(offlineA);
      await expect
        .poll(async () => await readEntryBody(pageB, entryPath), { timeout: 30000 })
        .toContain(offlineB);

      // Content should converge.
      const bodyA = await readEntryBody(pageA, entryPath, { sync: false });
      const bodyB = await readEntryBody(pageB, entryPath, { sync: false });
      expect(bodyA).toBe(bodyB);
    } finally {
      await contextA.setOffline(false).catch(() => undefined);
      await contextB.setOffline(false).catch(() => undefined);
      await Promise.allSettled([contextA.close(), contextB.close()]);
    }
  });
});
