/// <reference types="node" />

import {
  test,
  expect,
  waitForAppReady,
  clearAllBrowserStorage,
} from "./fixtures";
import type { Page } from "@playwright/test";
import { spawn, type ChildProcessWithoutNullStreams } from "child_process";
import path from "path";
import { fileURLToPath } from "url";
import { existsSync, mkdtempSync, rmSync } from "fs";
import { tmpdir } from "os";

const defaultServerHost = process.env.SYNC_SERVER_HOST ?? "127.0.0.1";
const baseServerPort = Number(process.env.SYNC_SERVER_PORT ?? "3030");
let serverPort = baseServerPort;
let serverUrl = process.env.SYNC_SERVER_URL ?? `http://${defaultServerHost}:${serverPort}`;
const shouldStartServer = process.env.SYNC_E2E_START_SERVER !== "0";
const repoRoot = path.resolve(
  fileURLToPath(new URL("../../..", import.meta.url)),
);

const syncServerBinary =
  process.env.SYNC_SERVER_BINARY ??
  path.join(repoRoot, "target/release/diaryx_sync_server");

let serverProcess: ChildProcessWithoutNullStreams | null = null;
let serverAvailable = false;
let tempDataDir: string | null = null;

function log(label: string, msg: string): void {
  console.log(`[${label}] ${msg}`);
}

function getProjectPort(projectName: string): number {
  switch (projectName) {
    case "webkit":
      return baseServerPort + 1;
    case "firefox":
      return baseServerPort + 2;
    case "chromium":
    default:
      return baseServerPort;
  }
}

async function waitForServerReady(): Promise<boolean> {
  log("server", "Waiting for server to be ready...");
  for (let attempt = 0; attempt < 40; attempt++) {
    try {
      const response = await fetch(`${serverUrl}/api/status`);
      if (response.ok) {
        log("server", `Server ready after ${attempt + 1} attempts`);
        return true;
      }
    } catch {
      // ignore and retry
    }
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  log("server", "Server failed to become ready after 40 attempts");
  return false;
}

function setupConsoleLogs(page: Page, label: string): void {
  page.on("console", (msg) => {
    const text = msg.text();
    if (
      text.includes("[Sync") ||
      text.includes("CRDT") ||
      text.includes("WebSocket") ||
      text.includes("sync") ||
      text.includes("Auth") ||
      text.includes("[App]") ||
      text.includes("[Storage]") ||
      text.includes("workspace") ||
      text.includes("error") ||
      text.includes("Error")
    ) {
      log(label, text);
    }
  });
}

async function enableShowAllFiles(page: Page): Promise<void> {
  await page.evaluate(async () => {
    const { workspaceStore } = await import("/src/models/stores");
    const { refreshTree } = await import("/src/controllers/workspaceController");
    const { getBackend, createApi } = await import("/src/lib/backend");

    workspaceStore.setShowUnlinkedFiles(true);
    const backend = await getBackend();
    const api = createApi(backend);
    await refreshTree(
      api,
      backend,
      workspaceStore.showUnlinkedFiles,
      workspaceStore.showHiddenFiles,
    );
  });
}

async function waitForWorkspaceCrdtInitialized(page: Page, timeout = 30000): Promise<void> {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    const initialized = await page.evaluate(async () => {
      const { workspaceStore } = await import("/src/models/stores");
      return workspaceStore.workspaceCrdtInitialized;
    });

    if (initialized) return;
    await page.waitForTimeout(500);
  }
  throw new Error("Timed out waiting for workspace CRDT to initialize");
}



async function waitForFileExists(page: Page, path: string, timeout = 30000): Promise<void> {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    const exists = await page.evaluate(async (entryPath) => {
      const { getBackend, createApi } = await import("/src/lib/backend");
      const { getFileMetadata } = await import("/src/lib/crdt/workspaceCrdtBridge");
      const backend = await getBackend();
      const api = createApi(backend);
      const candidates = [
        entryPath,
        entryPath.startsWith("./") ? entryPath.slice(2) : `./${entryPath}`,
      ];
      for (const candidate of candidates) {
        try {
          if (await api.fileExists(candidate)) {
            return true;
          }
        } catch {
          // ignore
        }
        try {
          const metadata = await getFileMetadata(candidate);
          if (metadata && !metadata.deleted) {
            return true;
          }
        } catch {
          // ignore
        }
      }
      return false;
    }, path);

    if (exists) return;
    await page.waitForTimeout(500);
  }
  throw new Error(`Timed out waiting for file to exist: ${path}`);
}

async function waitForEntryContent(
  page: Page,
  path: string,
  expected: string,
  timeout = 30000,
): Promise<void> {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    const content = await page.evaluate(async (entryPath) => {
      const { getBackend, createApi } = await import("/src/lib/backend");
      const { ensureBodySync, getBodyContentFromCrdt } = await import("/src/lib/crdt/workspaceCrdtBridge");
      const backend = await getBackend();
      const api = createApi(backend);
      const candidates = [
        entryPath,
        entryPath.startsWith("./") ? entryPath.slice(2) : `./${entryPath}`,
      ];
      for (const candidate of candidates) {
        try {
          await ensureBodySync(candidate);
          const crdtContent = await getBodyContentFromCrdt(candidate);
          if (crdtContent) {
            return crdtContent;
          }
        } catch {
          // ignore
        }
        try {
          const entry = await api.getEntry(candidate);
          if (entry?.content) {
            return entry.content;
          }
        } catch {
          // ignore
        }
        try {
          const content = await api.readFile(candidate);
          if (content) {
            return content;
          }
        } catch {
          // ignore
        }
      }
      return "";
    }, path);

    if (content.includes(expected)) return;
    await page.waitForTimeout(500);
  }
  throw new Error(`Timed out waiting for entry content in ${path}`);
}

async function createEntry(
  page: Page,
  entryPath: string,
  title: string,
  body: string,
  parentPath: string | null = null,
): Promise<string> {
  const resolvedPath = await page.evaluate(
    async ({ path, entryTitle, entryBody, parent }) => {
      const { getBackend, createApi } = await import("/src/lib/backend");
      const { refreshTree } = await import("/src/controllers/workspaceController");
      const { workspaceStore } = await import("/src/models/stores");

      const backend = await getBackend();
      const api = createApi(backend);
      // Resolve the actual root index so part_of points to the workspace root.
      let resolvedParent = parent;
      if (!resolvedParent) {
        const workspaceDir = backend
          .getWorkspacePath()
          .replace(/\/index\.md$/, "")
          .replace(/\/README\.md$/, "");
        try {
          resolvedParent = await api.findRootIndex(workspaceDir);
        } catch {
          resolvedParent = `${workspaceDir}/README.md`;
        }
      }
      // Link entry to parent so it's included in workspace exports
      await api.createEntry(path, { title: entryTitle, part_of: resolvedParent });
      await api.saveEntry(path, entryBody);

      // Also add to parent's contents array for export to work
      if (resolvedParent) {
        try {
          const parentFm = await api.getFrontmatter(resolvedParent);
          const contents = (parentFm.contents as string[]) || [];
          if (!contents.includes(path)) {
            contents.push(path);
            await api.setFrontmatterProperty(resolvedParent, "contents", contents);
          }
        } catch (e) {
          console.warn(`[test] Failed to add ${path} to ${resolvedParent} contents:`, e);
        }
      }

      let resolved = path;
      const candidates = [path];
      if (path.startsWith("./")) {
        candidates.push(path.slice(2));
      } else {
        candidates.push(`./${path}`);
      }
      for (const candidate of candidates) {
        try {
          if (await api.fileExists(candidate)) {
            resolved = candidate;
            break;
          }
        } catch {
          // ignore
        }
      }

      await refreshTree(
        api,
        backend,
        workspaceStore.showUnlinkedFiles,
        workspaceStore.showHiddenFiles,
      );
      return resolved;
    },
    { path: entryPath, entryTitle: title, entryBody: body, parent: parentPath },
  );

  await expect(
    page.getByRole("treeitem", { name: new RegExp(title) }),
  ).toBeVisible({ timeout: 15000 });

  return resolvedPath;
}

async function uploadWorkspaceSnapshot(
  page: Page,
  mode: "replace" | "merge" = "replace",
): Promise<void> {
  await page.evaluate(async (uploadMode) => {
    const { getBackend, createApi } = await import("/src/lib/backend");
    const { getCurrentWorkspace, uploadWorkspaceSnapshot } = await import(
      "/src/lib/auth/authStore.svelte"
    );
    const JSZip = (await import("jszip")).default;

    const backend = await getBackend();
    const api = createApi(backend);
    const workspace = getCurrentWorkspace();

    if (!workspace?.id) {
      throw new Error("No default workspace available for snapshot upload");
    }

    const workspaceDir = backend
      .getWorkspacePath()
      .replace(/\/index\.md$/, "")
      .replace(/\/README\.md$/, "");
    let workspacePath: string;
    try {
      workspacePath = await api.findRootIndex(workspaceDir);
    } catch {
      // Fall back to default README in workspace dir
      workspacePath = `${workspaceDir}/README.md`;
    }

    const zip = new JSZip();
    const files = await api.exportToMemory(workspacePath, "*");
    for (const file of files) {
      zip.file(file.path, file.content);
    }

    const blob = await zip.generateAsync({ type: "blob" });
    const result = await uploadWorkspaceSnapshot(workspace.id, blob, uploadMode);
    if (!result) {
      throw new Error("Snapshot upload failed");
    }
  }, mode);
}

async function waitForFileMissing(page: Page, path: string, timeout = 30000): Promise<void> {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    const exists = await page.evaluate(async (entryPath) => {
      const { getBackend, createApi } = await import("/src/lib/backend");
      const { getFileMetadata } = await import("/src/lib/crdt/workspaceCrdtBridge");
      const backend = await getBackend();
      const api = createApi(backend);
      const candidates = [
        entryPath,
        entryPath.startsWith("./") ? entryPath.slice(2) : `./${entryPath}`,
      ];
      for (const candidate of candidates) {
        try {
          if (await api.fileExists(candidate)) {
            return true;
          }
        } catch {
          // ignore
        }
        try {
          const metadata = await getFileMetadata(candidate);
          if (metadata && !metadata.deleted) {
            return true;
          }
        } catch {
          // ignore
        }
      }
      return false;
    }, path);

    if (!exists) return;
    await page.waitForTimeout(500);
  }
  throw new Error(`Timed out waiting for file to be deleted: ${path}`);
}

async function openSyncWizard(page: Page, label: string): Promise<void> {
  log(label, "Opening sync wizard");
  await page.getByLabel("Sync status").click();
  await page.getByRole("button", { name: /Set up sync|Manage sync/i }).click();
  await expect(page.getByText("Sign In to Sync")).toBeVisible({
    timeout: 10000,
  });
  log(label, "Sync wizard opened");
}

async function completeAuthAndInit(
  page: Page,
  email: string,
  modeLabel: RegExp,
  label: string,
): Promise<void> {
  await openSyncWizard(page, label);

  log(label, "Filling email and server URL");
  await page.getByLabel("Email Address").fill(email);
  const advancedButton = page.getByRole("button", { name: "Advanced" });
  await advancedButton.click({ force: true });
  await page.getByLabel("Server URL").fill(serverUrl);

  log(label, "Requesting magic link");
  const magicLinkResponsePromise = page.waitForResponse((response) => {
    return (
      response.url().includes("/auth/magic-link") &&
      response.request().method() === "POST"
    );
  });

  await page.getByRole("button", { name: /Send Sign-in Link/i }).click();

  const magicLinkResponse = await magicLinkResponsePromise;
  const magicLinkPayload = await magicLinkResponse.json();
  const devLink = magicLinkPayload?.dev_link;

  if (!devLink) {
    throw new Error(
      "Magic link dev link missing. Ensure the sync server email is not configured for tests.",
    );
  }

  const token = new URL(devLink).searchParams.get("token");
  if (!token) {
    throw new Error("Magic link token missing from dev link response.");
  }
  log(label, "Got magic link token");

  log(label, "Clicking dev link to verify token");
  // Click the dev link directly instead of injecting token into URL
  // This is more reliable than waiting for the wizard's URL polling
  const devLinkElement = page.locator('a:has-text("Click here to verify")');
  await devLinkElement.waitFor({ state: "visible", timeout: 10000 });
  await devLinkElement.click();

  log(label, "Waiting for token processing to complete");

  const modeButton = page.getByRole("button", { name: modeLabel });
  const startSyncButton = page.getByRole("button", { name: /Start Syncing/i });

  log(label, "Waiting for mode button to appear...");

  const syncStatusButton = page.getByLabel("Sync status");
  const popoverContent = page.locator('[data-slot="popover-content"]');

  // Helper to check if synced - only used after wizard closes
  async function isSynced(): Promise<boolean> {
    try {
      const expanded = await syncStatusButton.getAttribute("aria-expanded");
      if (expanded !== "true") {
        await syncStatusButton.click();
      }
    } catch {
      // ignore
    }

    const popoverSynced = await popoverContent
      .filter({ hasText: "Synced" })
      .isVisible()
      .catch(() => false);
    if (popoverSynced) return true;

    const syncText = await syncStatusButton.textContent().catch(() => "");
    return syncText?.includes("Synced") ?? false;
  }

  // Wait specifically for the mode button to appear
  // Don't check isSynced during this phase as it can interfere with the wizard
  try {
    await modeButton.waitFor({ state: "visible", timeout: 30000 });
    log(label, "Mode button is visible");
  } catch {
    // If mode button didn't appear, check if wizard is still open
    const dialogVisible = await page.getByRole("dialog").isVisible().catch(() => false);
    if (!dialogVisible) {
      // Wizard closed, check if already synced
      if (await isSynced()) {
        log(label, "Wizard closed, already synced");
        return;
      }
    }
    throw new Error(`${label}: Mode button "${modeLabel}" did not appear`);
  }

  await modeButton.click();
  log(label, `Init flow - clicked ${modeLabel}`);

  await startSyncButton.waitFor({ state: "visible", timeout: 5000 });
  await startSyncButton.click();
  log(label, "Clicked Start Syncing");

  log(label, "Waiting for sync to complete...");

  const syncingIndicator = page.getByText("Syncing...");
  try {
    await syncingIndicator.waitFor({ state: "visible", timeout: 5000 });
    log(label, "Sync started, waiting for completion...");
  } catch {
    log(label, "Syncing indicator not seen, sync may have completed quickly");
  }

  await page.waitForTimeout(500);
  await page.getByRole("dialog").waitFor({ state: "hidden", timeout: 30000 }).catch(() => {
    // Fallback: check sync status popover
  });

  const synced = await isSynced();
  if (!synced) {
    log(label, "Sync dialog closed but status not synced; waiting briefly");
    await page.waitForTimeout(2000);
  }
}

test.describe.serial("Sync Workspace Transfer", () => {
  test.beforeAll(async ({}, testInfo) => {
    // Skip server startup for WebKit - tests will be skipped anyway
    if (testInfo.project.name === 'webkit') {
      return;
    }

    if (!process.env.SYNC_SERVER_URL) {
      serverPort = getProjectPort(testInfo.project.name);
      serverUrl = `http://${defaultServerHost}:${serverPort}`;
    }

    if (shouldStartServer) {
      if (!existsSync(syncServerBinary)) {
        log("server", `Binary not found at: ${syncServerBinary}`);
        log(
          "server",
          "Build server first: cargo build --release -p diaryx_sync_server",
        );
        throw new Error(
          "Sync server binary not found. Build with: cargo build --release -p diaryx_sync_server",
        );
      }

      tempDataDir = mkdtempSync(path.join(tmpdir(), "diaryx-sync-test-"));
      log("server", `Using temp data dir: ${tempDataDir}`);

      log("server", `Starting server from binary: ${syncServerBinary}`);
      serverProcess = spawn(syncServerBinary, [], {
        cwd: repoRoot,
        stdio: "pipe",
        env: {
          ...process.env,
          DATABASE_PATH: path.join(tempDataDir, "test.db"),
          PORT: String(serverPort),
        },
      });

      serverProcess.stdout.on("data", (data) => {
        log("server", data.toString().trim());
      });
      serverProcess.stderr.on("data", (data) => {
        log("server-err", data.toString().trim());
      });

      serverProcess.on("error", (err) => {
        log("server", `Process error: ${err.message}`);
      });

      serverProcess.on("exit", (code, signal) => {
        log("server", `Process exited with code ${code}, signal ${signal}`);
      });
    }

    serverAvailable = await waitForServerReady();
  });

  test.afterAll(async () => {
    if (serverProcess) {
      log("server", "Stopping server");
      serverProcess.kill("SIGINT");
      serverProcess = null;
    }

    if (tempDataDir && existsSync(tempDataDir)) {
      log("server", `Cleaning up temp dir: ${tempDataDir}`);
      rmSync(tempDataDir, { recursive: true, force: true });
      tempDataDir = null;
    }
  });

  test("uploads workspace and replaces local content on another client", async ({ browser, browserName }) => {
    test.setTimeout(180000);
    test.skip(browserName === 'webkit', 'WebKit OPFS not fully supported');
    test.skip(!serverAvailable, "Sync server not available");

    log("test", "Creating browser contexts");
    const contextA = await browser.newContext();
    const contextB = await browser.newContext();

    const pageA = await contextA.newPage();
    const pageB = await contextB.newPage();

    setupConsoleLogs(pageA, "clientA");
    setupConsoleLogs(pageB, "clientB");

    log("test", "Navigating to app");
    await pageA.goto("/");
    await pageB.goto("/");

    log("clientA", "Clearing browser storage");
    await clearAllBrowserStorage(pageA);
    log("clientB", "Clearing browser storage");
    await clearAllBrowserStorage(pageB);

    log("test", "Reloading pages after storage clear");
    await pageA.reload();
    await pageB.reload();

    log("test", "Waiting for app ready");
    await waitForAppReady(pageA, 40000);
    await waitForAppReady(pageB, 40000);
    log("test", "Waiting for workspace CRDT initialization");
    await waitForWorkspaceCrdtInitialized(pageA);
    await waitForWorkspaceCrdtInitialized(pageB);

    log("test", "Enabling show all files mode");
    await enableShowAllFiles(pageA);
    await enableShowAllFiles(pageB);

    const runSuffix = Date.now();
    const testEmail = `sync-transfer-${runSuffix}@example.com`;
    log("test", `Using test email: ${testEmail}`);

    const entries = [
      {
        path: `alpha-${runSuffix}.md`,
        title: `Alpha ${runSuffix}`,
        body: `Alpha body ${runSuffix}`,
      },
      {
        path: `beta-${runSuffix}.md`,
        title: `Beta ${runSuffix}`,
        body: `Beta body ${runSuffix}`,
      },
    ];

    for (const entry of entries) {
      log("clientA", `Creating entry ${entry.title}`);
      entry.path = await createEntry(pageA, entry.path, entry.title, entry.body);
    }

    const localEntry = {
      path: `local-only-${runSuffix}.md`,
      title: `Local Only ${runSuffix}`,
      body: `Local body ${runSuffix}`,
    };

    log("clientB", `Creating local-only entry ${localEntry.title}`);
    localEntry.path = await createEntry(pageB, localEntry.path, localEntry.title, localEntry.body);

    log("test", "Starting auth for clientA");
    await completeAuthAndInit(pageA, testEmail, /Sync local content/i, "clientA");

    log("test", "Waiting for clientA sync to fully complete");
    await pageA.waitForTimeout(5000);

    // Debug: Check what files are in the CRDT after sync
    const crdtFiles = await pageA.evaluate(async () => {
      const { getAllFiles, getFileMetadata } = await import("/src/lib/crdt/workspaceCrdtBridge");
      const files = await getAllFiles();
      const result: { path: string; deleted: boolean; title?: string }[] = [];
      for (const [path, meta] of files.entries()) {
        result.push({
          path,
          deleted: meta.deleted ?? false,
          title: meta.title,
        });
      }
      return result;
    });
    log("test", `CRDT files after clientA sync: ${JSON.stringify(crdtFiles, null, 2)}`);

    // Debug: Check what files are on disk
    const diskFiles = await pageA.evaluate(async () => {
      const { getBackend, createApi } = await import("/src/lib/backend");
      const backend = await getBackend();
      const api = createApi(backend);
      try {
        const tree = await api.getFilesystemTree(".", true);
        const paths: string[] = [];
        const collectPaths = (node: any) => {
          paths.push(node.path);
          if (node.children) {
            for (const child of node.children) {
              collectPaths(child);
            }
          }
        };
        collectPaths(tree);
        return paths;
      } catch (e) {
        return [`Error: ${e}`];
      }
    });
    log("test", `Disk files after clientA sync: ${JSON.stringify(diskFiles, null, 2)}`);

    // Verify entries appear in clientA's tree after sync
    log("test", "Verifying entries appear in clientA tree");
    for (const entry of entries) {
      await expect(
        pageA.getByRole("treeitem", { name: new RegExp(entry.title) }),
      ).toBeVisible({ timeout: 15000 });
    }

    log("test", "Starting auth for clientB");
    await completeAuthAndInit(pageB, testEmail, /Load from server/i, "clientB");

    log("clientB", "Waiting for local-only entry to be removed after load from server");
    await waitForFileMissing(pageB, localEntry.path);

    for (const entry of entries) {
      log("clientB", `Waiting for server entry ${entry.title} to exist`);
      await waitForFileExists(pageB, entry.path);
      await waitForEntryContent(pageB, entry.path, entry.body);
    }

    log("test", "Closing contexts");
    await contextA.close();
    await contextB.close();
  });
});
