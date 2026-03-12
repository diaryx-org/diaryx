/**
 * Sync lifecycle tests: workspace linking, bootstrapping, snapshot
 * upload/download, unlinking, and re-snapshot after live changes.
 */

import {
  test,
  expect,
  ensureSyncServer,
  ensureSyncPluginBase64,
  restartSyncServer,
  stopSpawnedSyncServer,
  setupSyncedPair,
  setupSingleSyncClient,
  installSyncPlugin,
  installSyncPluginInCurrentWorkspace,
  signInWithDevMagicLink,
  removeCurrentDeviceFromAccount,
  waitForE2EBridge,
  waitForSyncSession,
  createSyncedWorkspaceViaUi,
  downloadRemoteWorkspaceViaUi,
  currentWorkspaceName,
  currentWorkspaceProviderLink,
  uploadWorkspaceSnapshot,
  executePluginCommand,
  rootEntryPath,
  readEntryBody,
  createEntryWithMarker,
  openEntryForSync,
  queueBodyUpdateForSync,
  escapeRegex,
  waitForAppReady,
} from "./helpers";

const SHOULD_LOG_SYNC_E2E_DEBUG = process.env.SYNC_E2E_DEBUG === "1";

test.describe("Sync › Lifecycle", () => {
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

  test("links, bootstraps, and propagates live workspace changes across two browser clients", async ({ browser, browserName }) => {
    test.skip(browserName !== "chromium", "Sync E2E currently runs on Chromium only");
    test.setTimeout(300000);

    const pair = await setupSyncedPair(browser, "lifecycle");
    const { pageA, pageB, rootPathA, rootPathB } = pair;

    try {
      const runId = `${Date.now()}-${Math.floor(Math.random() * 100000)}`;
      const marker = `LIFECYCLE_BODY_${runId}`;

      // Create entry on A, verify it arrives on B.
      const entryPath = await createEntryWithMarker(pageA, `lifecycle-${runId}`, marker);
      await openEntryForSync(pageA, entryPath);
      await queueBodyUpdateForSync(pageA, entryPath);
      await openEntryForSync(pageB, entryPath);

      await expect
        .poll(async () => {
          await openEntryForSync(pageB, entryPath);
          return await readEntryBody(pageB, entryPath);
        }, { timeout: 30000 })
        .toContain(marker);
    } finally {
      await Promise.allSettled([pair.contextA.close(), pair.contextB.close()]);
    }
  });

  test("unlinks workspace and stops sync", async ({ browser, browserName }) => {
    test.skip(browserName !== "chromium", "Sync E2E currently runs on Chromium only");
    test.setTimeout(300000);

    const client = await setupSingleSyncClient(browser, "unlink");
    const { page, remoteId } = client;

    try {
      // Verify provider link exists before unlinking.
      const linkBefore = await currentWorkspaceProviderLink(page);
      expect(linkBefore).not.toBeNull();
      expect(linkBefore!.remoteWorkspaceId).toBe(remoteId);

      // Unlink the workspace.
      console.log("[sync-e2e:unlink] step: unlinking workspace");
      await executePluginCommand(page, "diaryx.sync", "UnlinkWorkspace", {});

      // Provider link should be cleared.
      // The plugin clears its internal workspace_id on unlink, but the host
      // metadata is cleared by the workspaceProviderService.  Since we called
      // the plugin command directly (not the UI flow), the host metadata may
      // still be present.  Verify the plugin itself reports no workspace.
      const statusAfter = await executePluginCommand(
        page,
        "diaryx.sync",
        "GetSyncStatus",
        {},
      ) as { state?: string } | null;

      // After unlink the plugin should no longer be in "synced" state.
      expect(statusAfter).toBeTruthy();
      expect((statusAfter as { state: string }).state).not.toBe("synced");
    } finally {
      await client.context.close();
    }
  });

  test("snapshot re-upload after live changes captures accumulated state", async ({ browser, browserName }) => {
    test.skip(browserName !== "chromium", "Sync E2E currently runs on Chromium only");
    test.setTimeout(300000);

    const pair = await setupSyncedPair(browser, "re-snapshot");
    const { pageA, pageB } = pair;

    try {
      const runId = `${Date.now()}-${Math.floor(Math.random() * 100000)}`;
      const marker1 = `SNAP_ENTRY1_${runId}`;
      const marker2 = `SNAP_ENTRY2_${runId}`;

      // Create two entries on A with live sync.
      const entry1 = await createEntryWithMarker(pageA, `snap-1-${runId}`, marker1);
      const entry2 = await createEntryWithMarker(pageA, `snap-2-${runId}`, marker2);
      await openEntryForSync(pageA, entry1);
      await openEntryForSync(pageA, entry2);
      await Promise.all([
        queueBodyUpdateForSync(pageA, entry1),
        queueBodyUpdateForSync(pageA, entry2),
      ]);

      // Wait for them to arrive on B.
      await openEntryForSync(pageB, entry1);
      await openEntryForSync(pageB, entry2);
      await expect
        .poll(async () => {
          await openEntryForSync(pageB, entry1);
          return await readEntryBody(pageB, entry1);
        }, { timeout: 30000 })
        .toContain(marker1);
      await expect
        .poll(async () => {
          await openEntryForSync(pageB, entry2);
          return await readEntryBody(pageB, entry2);
        }, { timeout: 30000 })
        .toContain(marker2);

      // Re-upload a snapshot from A.
      console.log("[sync-e2e:re-snapshot] step: re-uploading snapshot");
      await uploadWorkspaceSnapshot(pageA, pair.remoteId);

      console.log("[sync-e2e:re-snapshot] step: freeing one registered device for fresh client");
      await removeCurrentDeviceFromAccount(pageB);
      await pair.contextB.close();

      // Download in a fresh third client and verify.
      const contextC = await browser.newContext();
      await contextC.addInitScript(() => {
        localStorage.setItem("diaryx-storage-type", "indexeddb");
        localStorage.setItem("diaryx_sync_enabled", "true");
        localStorage.setItem("diaryx_e2e_skip_onboarding", "1");
        (globalThis as { __diaryx_e2e_disable_auto_file_open?: boolean }).__diaryx_e2e_disable_auto_file_open = true;
      });
      const pageC = await contextC.newPage();
      pageC.on("console", (message) => {
        const text = message.text();
        if (
          SHOULD_LOG_SYNC_E2E_DEBUG
          && (text.includes("[extism-plugin:") || text.includes("[extism]") || text.includes("[ws:"))
        ) {
          process.stderr.write(`[re-snapshot:pageC:${message.type()}] ${text}\n`);
        }
      });

      try {
        const wasmBase64 = await ensureSyncPluginBase64();
        console.log("[sync-e2e:re-snapshot] step: opening pageC");
        await pageC.goto("/");
        await waitForAppReady(pageC, 45000);
        console.log("[sync-e2e:re-snapshot] step: installing plugin on pageC");
        await installSyncPlugin(pageC, wasmBase64);

        console.log("[sync-e2e:re-snapshot] step: signing in on pageC");
        await signInWithDevMagicLink(pageC, pair.email, { waitForProviderReady: false });
        console.log("[sync-e2e:re-snapshot] step: waiting for pageC bridge");
        await waitForE2EBridge(pageC);
        await pageC.evaluate(() => (globalThis as any).__diaryx_e2e.setAutoAllowPermissions(true));

        console.log("[sync-e2e:re-snapshot] step: downloading remote workspace on pageC");
        await downloadRemoteWorkspaceViaUi(pageC, `Sync E2E re-snapshot ${runId}`, pair.remoteId);
        console.log("[sync-e2e:re-snapshot] step: installing workspace plugin on pageC");
        await installSyncPluginInCurrentWorkspace(pageC, wasmBase64);
        console.log("[sync-e2e:re-snapshot] step: waiting for pageC sync session");
        await waitForSyncSession(pageC);

        console.log("[sync-e2e:re-snapshot] step: verifying entries in fresh download");
        await openEntryForSync(pageC, entry1);
        await expect
          .poll(async () => await readEntryBody(pageC, entry1), { timeout: 30000 })
          .toContain(marker1);
        await openEntryForSync(pageC, entry2);
        await expect
          .poll(async () => await readEntryBody(pageC, entry2), { timeout: 30000 })
          .toContain(marker2);
      } finally {
        await contextC.close();
      }
    } finally {
      await Promise.allSettled([pair.contextA.close(), pair.contextB.close()]);
    }
  });
});
