/**
 * Sync offline/reconnect tests: verifies that changes made while offline
 * are queued and synced when the connection is restored.
 *
 * The sync plugin uses LWW (last-writer-wins) conflict resolution:
 * - When one client edits offline, its changes push on reconnect.
 * - When both clients edit offline, only the most recent write survives.
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
  triggerSync,
  waitForSyncSettled,
} from "./helpers";

test.describe("Sync > Offline / Reconnect", () => {
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
      await expect
        .poll(async () => await readEntryBody(pageB, entryPath), { timeout: 30000 })
        .toContain(initialMarker);

      // Take client A offline.
      console.log("[sync-e2e:offline] step: taking pageA offline");
      await contextA.setOffline(true);

      // Make edits on A while offline (these are saved locally but can't push).
      await appendMarkerToEntry(pageA, entryPath, offlineMarkerA);
      await setFrontmatterProperty(pageA, entryPath, "description", offlineDesc);

      // Verify A has the changes locally.
      await expect
        .poll(async () => await readEntryBody(pageA, entryPath, { sync: false }), { timeout: 10000 })
        .toContain(offlineMarkerA);

      // B should NOT see the offline changes yet.
      await pageB.waitForTimeout(3000);
      const bodyBWhileOffline = await readEntryBody(pageB, entryPath, { sync: false });
      expect(bodyBWhileOffline).not.toContain(offlineMarkerA);

      // Bring A back online.
      console.log("[sync-e2e:offline] step: bringing pageA back online");
      await contextA.setOffline(false);

      // Trigger sync on A to push the offline edits.
      await triggerSync(pageA);

      // B should eventually see the changes that were queued offline.
      console.log("[sync-e2e:offline] step: waiting for offline edits to propagate to pageB");
      await expect
        .poll(async () => await readEntryBody(pageB, entryPath), { timeout: 45000 })
        .toContain(offlineMarkerA);
      await expectFrontmatterProperty(pageB, entryPath, "description", offlineDesc, 30000);
    } finally {
      await contextA.setOffline(false).catch(() => undefined);
      await Promise.allSettled([contextA.close(), contextB.close()]);
    }
  });

  test("last writer wins when both clients edit offline", async ({ browser, browserName }) => {
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
      // Small delay so B's edit has a later timestamp (LWW).
      await pageB.waitForTimeout(1000);
      await appendMarkerToEntry(pageB, entryPath, offlineB);

      // Verify local state.
      await expect
        .poll(async () => await readEntryBody(pageA, entryPath, { sync: false }), { timeout: 10000 })
        .toContain(offlineA);
      await expect
        .poll(async () => await readEntryBody(pageB, entryPath, { sync: false }), { timeout: 10000 })
        .toContain(offlineB);

      // Bring A online first, let it push.
      console.log("[sync-e2e:dual-offline] step: bringing A back online");
      await contextA.setOffline(false);
      await triggerSync(pageA);

      // Bring B online — B's edit has a later timestamp, so it should win.
      console.log("[sync-e2e:dual-offline] step: bringing B back online");
      await contextB.setOffline(false);
      await triggerSync(pageB);

      // After B pushes, both clients should converge.
      // B was the last writer, so its content should win.
      console.log("[sync-e2e:dual-offline] step: waiting for convergence");
      await expect
        .poll(async () => await readEntryBody(pageB, entryPath), { timeout: 45000 })
        .toContain(offlineB);

      // A should pull B's version.
      await expect
        .poll(async () => await readEntryBody(pageA, entryPath), { timeout: 30000 })
        .toContain(offlineB);

      // Both clients should have the same content.
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
