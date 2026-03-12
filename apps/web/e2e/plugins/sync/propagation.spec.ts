/**
 * Sync propagation tests: entry CRUD operations syncing between two clients,
 * concurrent edits (body + frontmatter), and bidirectional sync.
 */

import {
  test,
  expect,
  ensureSyncServer,
  ensureSyncPluginBase64,
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
  openEntryForSync,
  rootEntryPath,
  uploadWorkspaceSnapshot,
  waitForAppReady,
} from "./helpers";

test.describe("Sync › Propagation", () => {
  test.describe.configure({ mode: "serial" });

  test.beforeAll(async ({ browserName }) => {
    if (browserName !== "chromium") return;
    await ensureSyncServer();
    await ensureSyncPluginBase64();
  });

  test.afterAll(async ({ browserName }) => {
    if (browserName !== "chromium") return;
    await stopSpawnedSyncServer();
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
      await openEntryForSync(pageA, createdEntryPath);
      await openEntryForSync(pageB, createdEntryPath);
      await expect
        .poll(async () => await readEntryBody(pageB, createdEntryPath), { timeout: 30000 })
        .toContain(createdEntryMarker);
      await openEntryForSync(pageB, rootPathB);

      // --- Rename ---
      const renamedEntryPath = await renameEntry(
        pageA,
        createdEntryPath,
        renamedEntryFilename,
      );

      // --- Move ---
      const destinationParentPath = await createIndexEntry(pageA, destinationParentStem);
      const destinationContentsBeforeMove = await readFrontmatterProperty(
        pageA,
        destinationParentPath,
        "contents",
      );

      const movedEntryPath = await moveEntryToParent(
        pageA,
        renamedEntryPath,
        destinationParentPath,
      );
      await openEntryForSync(pageA, movedEntryPath);
      await openEntryForSync(pageB, movedEntryPath);

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

      const initialDestinationContentsCount = Array.isArray(destinationContentsBeforeMove)
        ? destinationContentsBeforeMove.length
        : 0;
      await expect
        .poll(async () => {
          const contents = await readFrontmatterProperty(pageA, destinationParentPath, "contents");
          return Array.isArray(contents) ? contents.length : 0;
        }, { timeout: 30000 })
        .toBeGreaterThan(initialDestinationContentsCount);
      const expectedDestinationContents = await readFrontmatterProperty(
        pageA,
        destinationParentPath,
        "contents",
      );

      await expectFrontmatterProperty(pageB, movedEntryPath, "part_of", expectedMovedPartOf);
      await expectFrontmatterProperty(pageB, destinationParentPath, "contents", expectedDestinationContents);

      // --- Frontmatter update ---
      console.log("[sync-e2e:crud] step: set frontmatter description");
      await setFrontmatterProperty(pageA, movedEntryPath, "description", descriptionMarker);
      await expect
        .poll(async () => (await readFrontmatter(pageB, movedEntryPath))?.description ?? null, {
          timeout: 30000,
        })
        .toBe(descriptionMarker);

      // --- Body edit (live, both subscribed) ---
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
      await openEntryForSync(pageB, movedEntryPath);
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
      await openEntryForSync(pageA, entryPath);
      await openEntryForSync(pageB, entryPath);
      await expect
        .poll(async () => await readEntryBody(pageB, entryPath), { timeout: 30000 })
        .toContain(`DELETE_ME_${runId}`);

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
      await openEntryForSync(pageB, entryPath);
      await setFrontmatterProperty(pageB, entryPath, "description", fmValue);

      await openEntryForSync(pageA, entryPath);
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

  test("merges concurrent body edits from both clients without data loss", async ({ browser, browserName }) => {
    test.skip(browserName !== "chromium", "Sync E2E currently runs on Chromium only");
    test.setTimeout(300000);

    const pair = await setupSyncedPair(browser, "concurrent-body");
    const { pageA, pageB } = pair;

    try {
      const runId = `${Date.now()}-${Math.floor(Math.random() * 100000)}`;
      const initialMarker = `INITIAL_${runId}`;
      const markerA = `EDIT_FROM_A_${runId}`;
      const markerB = `EDIT_FROM_B_${runId}`;

      const entryPath = await createEntryWithMarker(
        pageA,
        `concurrent-${runId}`,
        initialMarker,
      );
      await openEntryForSync(pageA, entryPath);
      await openEntryForSync(pageB, entryPath);
      await expect
        .poll(async () => await readEntryBody(pageB, entryPath), { timeout: 30000 })
        .toContain(initialMarker);

      console.log("[sync-e2e:concurrent] step: concurrent edits");
      await Promise.all([
        appendMarkerToEntry(pageA, entryPath, markerA),
        appendMarkerToEntry(pageB, entryPath, markerB),
      ]);

      console.log("[sync-e2e:concurrent] step: waiting for merged content on both clients");
      await expect
        .poll(async () => await readEntryBody(pageA, entryPath), { timeout: 30000 })
        .toContain(markerA);
      await expect
        .poll(async () => await readEntryBody(pageA, entryPath), { timeout: 30000 })
        .toContain(markerB);

      await expect
        .poll(async () => await readEntryBody(pageB, entryPath), { timeout: 30000 })
        .toContain(markerA);
      await expect
        .poll(async () => await readEntryBody(pageB, entryPath), { timeout: 30000 })
        .toContain(markerB);

      // Both clients should converge to the same final content.
      const bodyA = await readEntryBody(pageA, entryPath);
      const bodyB = await readEntryBody(pageB, entryPath);
      expect(bodyA).toBe(bodyB);
    } finally {
      await Promise.allSettled([pair.contextA.close(), pair.contextB.close()]);
    }
  });

  test("concurrent frontmatter edits to different keys merge correctly", async ({ browser, browserName }) => {
    test.skip(browserName !== "chromium", "Sync E2E currently runs on Chromium only");
    test.setTimeout(300000);

    const pair = await setupSyncedPair(browser, "concurrent-fm");
    const { pageA, pageB } = pair;

    try {
      const runId = `${Date.now()}-${Math.floor(Math.random() * 100000)}`;
      const tagValueA = `tag-from-a-${runId}`;
      const descValueB = `desc-from-b-${runId}`;

      const entryPath = await createEntryWithMarker(
        pageA,
        `fm-concurrent-${runId}`,
        `FM_BODY_${runId}`,
      );
      await openEntryForSync(pageA, entryPath);
      await openEntryForSync(pageB, entryPath);
      await expect
        .poll(async () => await readEntryBody(pageB, entryPath), { timeout: 30000 })
        .toContain(`FM_BODY_${runId}`);

      console.log("[sync-e2e:fm-concurrent] step: concurrent frontmatter edits");
      await Promise.all([
        setFrontmatterProperty(pageA, entryPath, "tags", [tagValueA]),
        setFrontmatterProperty(pageB, entryPath, "description", descValueB),
      ]);

      console.log("[sync-e2e:fm-concurrent] step: waiting for merged frontmatter");
      await expectFrontmatterProperty(pageA, entryPath, "description", descValueB);
      await expectFrontmatterProperty(pageA, entryPath, "tags", [tagValueA]);

      await expectFrontmatterProperty(pageB, entryPath, "description", descValueB);
      await expectFrontmatterProperty(pageB, entryPath, "tags", [tagValueA]);
    } finally {
      await Promise.allSettled([pair.contextA.close(), pair.contextB.close()]);
    }
  });
});
