/**
 * Sync attachment tests: attachment upload/download propagation between
 * two synced clients, and attachment metadata sync.
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
  openEntryForSync,
  readEntryBody,
  readFrontmatter,
  uploadAttachment,
  getAttachments,
  getAttachmentData,
  allowPermissionPrompts,
} from "./helpers";

test.describe("Sync › Attachments", () => {
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

  test("attachment metadata syncs between clients after upload", async ({ browser, browserName }) => {
    test.skip(browserName !== "chromium", "Sync E2E currently runs on Chromium only");
    test.setTimeout(300000);

    const pair = await setupSyncedPair(browser, "attach-meta");
    const { pageA, pageB } = pair;

    try {
      const runId = `${Date.now()}-${Math.floor(Math.random() * 100000)}`;

      // Create an entry on A.
      const entryPath = await createEntryWithMarker(
        pageA,
        `attach-${runId}`,
        `ATTACH_BODY_${runId}`,
      );
      await openEntryForSync(pageA, entryPath);
      await openEntryForSync(pageB, entryPath);

      // Wait for B to see the entry body.
      await expect
        .poll(async () => await readEntryBody(pageB, entryPath), { timeout: 30000 })
        .toContain(`ATTACH_BODY_${runId}`);

      // Upload a small text file as attachment on A.
      const content = `Hello attachment ${runId}`;
      const dataBase64 = Buffer.from(content).toString("base64");
      const filename = `test-${runId}.txt`;

      console.log("[sync-e2e:attach] step: uploading attachment on pageA");
      const attachmentPath = await uploadAttachment(pageA, entryPath, filename, dataBase64);
      expect(attachmentPath).toBeTruthy();

      // Verify the attachment is registered on A.
      const attachmentsA = await getAttachments(pageA, entryPath);
      expect(attachmentsA.length).toBeGreaterThan(0);

      // Verify attachment metadata propagates to B via frontmatter sync.
      // The attachment registration updates the entry's frontmatter with
      // binary_refs, which should sync through the CRDT.
      console.log("[sync-e2e:attach] step: waiting for attachment metadata on pageB");
      await expect
        .poll(async () => {
          const fm = await readFrontmatter(pageB, entryPath);
          const refs = fm?.binary_refs ?? fm?.attachments;
          return Array.isArray(refs) ? refs.length : 0;
        }, { timeout: 30000 })
        .toBeGreaterThan(0);

      // Verify A can read the attachment data back correctly.
      const dataBack = await getAttachmentData(pageA, entryPath, attachmentPath);
      const contentBack = Buffer.from(dataBack).toString("utf-8");
      expect(contentBack).toBe(content);
    } finally {
      await Promise.allSettled([pair.contextA.close(), pair.contextB.close()]);
    }
  });

  test("multiple attachments on different entries sync independently", async ({ browser, browserName }) => {
    test.skip(browserName !== "chromium", "Sync E2E currently runs on Chromium only");
    test.setTimeout(300000);

    const pair = await setupSyncedPair(browser, "attach-multi");
    const { pageA, pageB } = pair;

    try {
      const runId = `${Date.now()}-${Math.floor(Math.random() * 100000)}`;

      // Create two entries on A.
      const entry1 = await createEntryWithMarker(pageA, `attach-multi-1-${runId}`, `BODY1_${runId}`);
      const entry2 = await createEntryWithMarker(pageA, `attach-multi-2-${runId}`, `BODY2_${runId}`);
      await openEntryForSync(pageA, entry1);
      await openEntryForSync(pageA, entry2);
      await openEntryForSync(pageB, entry1);
      await openEntryForSync(pageB, entry2);

      // Wait for both entries on B.
      await expect
        .poll(async () => await readEntryBody(pageB, entry1), { timeout: 30000 })
        .toContain(`BODY1_${runId}`);
      await expect
        .poll(async () => await readEntryBody(pageB, entry2), { timeout: 30000 })
        .toContain(`BODY2_${runId}`);

      // Upload distinct attachments to each entry.
      const data1 = Buffer.from(`File one ${runId}`).toString("base64");
      const data2 = Buffer.from(`File two ${runId}`).toString("base64");

      const attach1 = await uploadAttachment(pageA, entry1, `one-${runId}.txt`, data1);
      const attach2 = await uploadAttachment(pageA, entry2, `two-${runId}.txt`, data2);

      // Verify each entry has exactly its own attachment.
      const attachments1 = await getAttachments(pageA, entry1);
      const attachments2 = await getAttachments(pageA, entry2);
      expect(attachments1.length).toBeGreaterThan(0);
      expect(attachments2.length).toBeGreaterThan(0);

      // Verify metadata propagates to B for both entries.
      console.log("[sync-e2e:attach-multi] step: waiting for metadata on pageB");
      await expect
        .poll(async () => {
          const fm = await readFrontmatter(pageB, entry1);
          const refs = fm?.binary_refs ?? fm?.attachments;
          return Array.isArray(refs) ? refs.length : 0;
        }, { timeout: 30000 })
        .toBeGreaterThan(0);
      await expect
        .poll(async () => {
          const fm = await readFrontmatter(pageB, entry2);
          const refs = fm?.binary_refs ?? fm?.attachments;
          return Array.isArray(refs) ? refs.length : 0;
        }, { timeout: 30000 })
        .toBeGreaterThan(0);
    } finally {
      await Promise.allSettled([pair.contextA.close(), pair.contextB.close()]);
    }
  });
});
