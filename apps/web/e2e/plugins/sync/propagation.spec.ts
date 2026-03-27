/**
 * Sync propagation tests: entry CRUD operations syncing between two clients
 * via the LWW file sync engine.
 *
 * Note: The sync plugin uses last-writer-wins (LWW) conflict resolution.
 * Concurrent edits to the same file are NOT merged — the most recent write
 * wins. These tests verify unidirectional and sequential propagation only.
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
  createIndexEntry,
  appendMarkerToEntry,
  renameEntry,
  moveEntryToParent,
  deleteEntry,
  setFrontmatterProperty,
  readEntryBody,
  readFrontmatter,
  readFrontmatterProperty,
  expectFrontmatterProperty,
  entryExists,
  triggerSync,
  rootEntryPath,
  uploadWorkspaceSnapshot,
  waitForAppReady,
} from "./helpers";

test.describe("Sync > Propagation", () => {
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

  test("propagates entry creation, rename, move, edit, and frontmatter across clients", async ({ browser, browserName }) => {
    test.skip(browserName !== "chromium", "Sync E2E currently runs on Chromium only");
    test.setTimeout(300000);

    const pair = await setupSyncedPair(browser, "crud");
    const { pageA, pageB, rootPathB } = pair;

    try {
      const runId = `${Date.now()}-${Math.floor(Math.random() * 100000)}`;
      const createdEntryStem = `live-propagation-${runId}`;
      const createdEntryMarker = `CREATED_BODY_${runId}`;
      const renamedEntryFilename = `live-propagation-renamed-${runId}.md`;
      const destinationParentStem = `move-destination-${runId}`;
      const descriptionMarker = `DESCRIPTION_${runId}`;
      const editedBodyMarker = `EDITED_BODY_${runId}`;

      // --- Create ---
      const createdEntryPath = await createEntryWithMarker(
        pageA,
        createdEntryStem,
        createdEntryMarker,
      );
      await expect
        .poll(async () => await readEntryBody(pageB, createdEntryPath), { timeout: 30000 })
        .toContain(createdEntryMarker);

      // --- Rename ---
      const renamedEntryPath = await renameEntry(
        pageA,
        createdEntryPath,
        renamedEntryFilename,
      );
      // Push rename to server
      await triggerSync(pageA);

      // --- Move ---
      const destinationParentPath = await createIndexEntry(pageA, destinationParentStem);
      const movedEntryPath = await moveEntryToParent(
        pageA,
        renamedEntryPath,
        destinationParentPath,
      );
      // Push move to server
      await triggerSync(pageA);

      console.log("[sync-e2e:crud] step: check body after move");
      await expect
        .poll(async () => await readEntryBody(pageB, movedEntryPath), { timeout: 30000 })
        .toContain(createdEntryMarker);

      // Verify part_of and contents frontmatter propagate.
      await expect
        .poll(async () => await readFrontmatterProperty(pageA, movedEntryPath, "part_of"), {
          timeout: 30000,
        })
        .not.toBeNull();
      const expectedMovedPartOf = await readFrontmatterProperty(pageA, movedEntryPath, "part_of");
      await expectFrontmatterProperty(pageB, movedEntryPath, "part_of", expectedMovedPartOf);

      // --- Frontmatter update ---
      console.log("[sync-e2e:crud] step: set frontmatter description");
      await setFrontmatterProperty(pageA, movedEntryPath, "description", descriptionMarker);
      await expect
        .poll(async () => (await readFrontmatter(pageB, movedEntryPath))?.description ?? null, {
          timeout: 30000,
        })
        .toBe(descriptionMarker);

      // --- Body edit ---
      await appendMarkerToEntry(pageA, movedEntryPath, editedBodyMarker);
      await expect
        .poll(async () => await readEntryBody(pageA, movedEntryPath), { timeout: 10000 })
        .toContain(editedBodyMarker);
      await expect
        .poll(async () => await readEntryBody(pageB, movedEntryPath), { timeout: 30000 })
        .toContain(editedBodyMarker);

      // --- Persistence after reload ---
      const expectedBodyAfterEdit = await readEntryBody(pageB, movedEntryPath);
      if (!expectedBodyAfterEdit) {
        throw new Error("Expected body content on page B before reload");
      }

      console.log("[sync-e2e:crud] step: reload pageB");
      await pageB.reload();
      await waitForAppReady(pageB, 45000);

      await expect
        .poll(async () => await pageB.evaluate(() => !!(globalThis as any).__diaryx_e2e), { timeout: 30000 })
        .toBe(true);
      await expect
        .poll(async () => await readEntryBody(pageB, movedEntryPath), { timeout: 30000 })
        .toBe(expectedBodyAfterEdit);
    } finally {
      await Promise.allSettled([pair.contextA.close(), pair.contextB.close()]);
    }
  });

  test("propagates entry deletion from one client to the other", async ({ browser, browserName }) => {
    test.skip(browserName !== "chromium", "Sync E2E currently runs on Chromium only");
    test.setTimeout(300000);

    const pair = await setupSyncedPair(browser, "delete");
    const { pageA, pageB } = pair;

    try {
      const runId = `${Date.now()}-${Math.floor(Math.random() * 100000)}`;

      const entryPath = await createEntryWithMarker(
        pageA,
        `delete-target-${runId}`,
        `DELETE_ME_${runId}`,
      );
      await expect
        .poll(async () => await entryExists(pageB, entryPath), { timeout: 30000 })
        .toBe(true);

      console.log("[sync-e2e:delete] step: deleting entry on pageA");
      const deleted = await deleteEntry(pageA, entryPath);
      expect(deleted).toBe(true);

      await expect
        .poll(async () => await entryExists(pageA, entryPath), { timeout: 15000 })
        .toBe(false);

      console.log("[sync-e2e:delete] step: waiting for deletion to propagate to pageB");
      await expect
        .poll(async () => await entryExists(pageB, entryPath), { timeout: 30000 })
        .toBe(false);
    } finally {
      await Promise.allSettled([pair.contextA.close(), pair.contextB.close()]);
    }
  });

  test("propagates entries created on client B back to client A (reverse direction)", async ({ browser, browserName }) => {
    test.skip(browserName !== "chromium", "Sync E2E currently runs on Chromium only");
    test.setTimeout(300000);

    const pair = await setupSyncedPair(browser, "reverse");
    const { pageA, pageB } = pair;

    try {
      const runId = `${Date.now()}-${Math.floor(Math.random() * 100000)}`;
      const marker = `REVERSE_BODY_${runId}`;
      const fmValue = `reverse-desc-${runId}`;

      console.log("[sync-e2e:reverse] step: creating entry on pageB");
      const entryPath = await createEntryWithMarker(
        pageB,
        `reverse-entry-${runId}`,
        marker,
      );
      await setFrontmatterProperty(pageB, entryPath, "description", fmValue);

      console.log("[sync-e2e:reverse] step: waiting for body on pageA");
      await expect
        .poll(async () => await readEntryBody(pageA, entryPath), { timeout: 30000 })
        .toContain(marker);

      console.log("[sync-e2e:reverse] step: waiting for frontmatter on pageA");
      await expectFrontmatterProperty(pageA, entryPath, "description", fmValue);
    } finally {
      await Promise.allSettled([pair.contextA.close(), pair.contextB.close()]);
    }
  });

  test("last-writer-wins when both clients edit the same file sequentially", async ({ browser, browserName }) => {
    test.skip(browserName !== "chromium", "Sync E2E currently runs on Chromium only");
    test.setTimeout(300000);

    const pair = await setupSyncedPair(browser, "lww");
    const { pageA, pageB } = pair;

    try {
      const runId = `${Date.now()}-${Math.floor(Math.random() * 100000)}`;
      const initialMarker = `INITIAL_${runId}`;
      const markerA = `EDIT_FROM_A_${runId}`;
      const markerB = `EDIT_FROM_B_${runId}`;

      const entryPath = await createEntryWithMarker(
        pageA,
        `lww-${runId}`,
        initialMarker,
      );

      // Wait for B to see the entry.
      await expect
        .poll(async () => await readEntryBody(pageB, entryPath), { timeout: 30000 })
        .toContain(initialMarker);

      // A edits and syncs first.
      await appendMarkerToEntry(pageA, entryPath, markerA);

      // Wait for A's edit to reach B.
      await expect
        .poll(async () => await readEntryBody(pageB, entryPath), { timeout: 30000 })
        .toContain(markerA);

      // Now B edits (this becomes the last writer).
      console.log("[sync-e2e:lww] step: B edits after A");
      await appendMarkerToEntry(pageB, entryPath, markerB);

      // A should eventually see B's version (which includes both markers
      // since B appended to the content it pulled from A).
      await expect
        .poll(async () => await readEntryBody(pageA, entryPath), { timeout: 30000 })
        .toContain(markerB);

      // Both clients should converge to the same content.
      const bodyA = await readEntryBody(pageA, entryPath);
      const bodyB = await readEntryBody(pageB, entryPath);
      expect(bodyA).toBe(bodyB);
    } finally {
      await Promise.allSettled([pair.contextA.close(), pair.contextB.close()]);
    }
  });

  test("frontmatter updates propagate sequentially between clients", async ({ browser, browserName }) => {
    test.skip(browserName !== "chromium", "Sync E2E currently runs on Chromium only");
    test.setTimeout(300000);

    const pair = await setupSyncedPair(browser, "fm-seq");
    const { pageA, pageB } = pair;

    try {
      const runId = `${Date.now()}-${Math.floor(Math.random() * 100000)}`;
      const tagValueA = `tag-from-a-${runId}`;
      const descValueB = `desc-from-b-${runId}`;

      const entryPath = await createEntryWithMarker(
        pageA,
        `fm-seq-${runId}`,
        `FM_BODY_${runId}`,
      );

      // Wait for B to see the entry.
      await expect
        .poll(async () => await readEntryBody(pageB, entryPath), { timeout: 30000 })
        .toContain(`FM_BODY_${runId}`);

      // A sets tags.
      console.log("[sync-e2e:fm-seq] step: A sets tags");
      await setFrontmatterProperty(pageA, entryPath, "tags", [tagValueA]);
      await expectFrontmatterProperty(pageB, entryPath, "tags", [tagValueA]);

      // B sets description (after pulling A's tags).
      console.log("[sync-e2e:fm-seq] step: B sets description");
      await setFrontmatterProperty(pageB, entryPath, "description", descValueB);
      await expectFrontmatterProperty(pageA, entryPath, "description", descValueB);

      // Both should have both properties.
      await expectFrontmatterProperty(pageA, entryPath, "tags", [tagValueA]);
      await expectFrontmatterProperty(pageB, entryPath, "tags", [tagValueA]);
    } finally {
      await Promise.allSettled([pair.contextA.close(), pair.contextB.close()]);
    }
  });
});
