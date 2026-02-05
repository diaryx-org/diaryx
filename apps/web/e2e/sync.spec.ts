/// <reference types="node" />

import {
  test,
  expect,
  EditorHelper,
  waitForAppReady,
  clearAllBrowserStorage,
} from "./fixtures";
import type { Page } from "@playwright/test";
import { spawn, type ChildProcessWithoutNullStreams } from "child_process";
import path from "path";
import { fileURLToPath } from "url";
import { existsSync, mkdtempSync, rmSync } from "fs";
import { tmpdir } from "os";

// Use IPv4 localhost by default to avoid IPv6 binding issues in Node environments
const serverUrl = process.env.SYNC_SERVER_URL ?? "http://127.0.0.1:3030";
const shouldStartServer = process.env.SYNC_E2E_START_SERVER !== "0";
const repoRoot = path.resolve(
  fileURLToPath(new URL("../../..", import.meta.url)),
);

// Use pre-built binary instead of cargo run
const syncServerBinary =
  process.env.SYNC_SERVER_BINARY ??
  path.join(repoRoot, "target/release/diaryx_sync_server");

let serverProcess: ChildProcessWithoutNullStreams | null = null;
let serverAvailable = false;
let tempDataDir: string | null = null;

function log(label: string, msg: string): void {
  console.log(`[${label}] ${msg}`);
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
    // Forward sync-related logs to test output
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

async function openSyncWizard(page: Page, label: string): Promise<void> {
  log(label, "Opening sync wizard");
  // Use getByLabel to avoid strict mode violation (popover trigger also matches)
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
  await page.getByRole("button", { name: "Advanced" }).click();
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

  // Inject token into URL via replaceState (wizard polls URL for token)
  log(label, "Injecting token into URL");
  await page.evaluate((tokenValue) => {
    const url = new URL(window.location.href);
    url.searchParams.set("token", tokenValue);
    window.history.replaceState({}, "", url);
  }, token);

  // Wait for token to be processed
  log(label, "Waiting for token processing to complete");

  const modeButton = page.getByRole("button", { name: modeLabel });
  const startSyncButton = page.getByRole("button", { name: /Start Syncing/i });

  // Wait for either mode selection step OR auto-sync completion
  // The app may auto-sync if server already has data (e.g., clientB joining clientA's workspace)
  log(label, "Waiting for mode selection or auto-sync...");

  // The "Synced" indicator appears in the sync status popover - look for it more specifically
  const syncStatusButton = page.getByLabel("Sync status");
  const initDialog = page.getByRole("dialog", { name: /Initialize Workspace/i });
  const popoverContent = page.locator('[data-slot="popover-content"]');

  // Helper to check if synced
  async function isSynced(): Promise<boolean> {
    // Open the popover to read the explicit status label
    try {
      const expanded = await syncStatusButton.getAttribute("aria-expanded");
      if (expanded !== "true") {
        await syncStatusButton.click();
      }
    } catch {
      // Ignore click errors; fallback to text checks below
    }

    // Prefer the popover status label (more reliable than button text)
    const popoverSynced = await popoverContent
      .filter({ hasText: "Synced" })
      .isVisible()
      .catch(() => false);
    if (popoverSynced) return true;

    // Fallback: check sync status button text
    const syncText = await syncStatusButton.textContent().catch(() => "");
    return syncText?.includes("Synced") ?? false;
  }

  // Poll for either mode button or synced state
  const startTime = Date.now();
  const timeout = 30000;
  let result: "mode_button" | "auto_synced" | "timeout" = "timeout";

  while (Date.now() - startTime < timeout) {
    // Check if mode button is visible
    if (await modeButton.isVisible().catch(() => false)) {
      result = "mode_button";
      break;
    }

    // Check if already synced
    if (await isSynced()) {
      result = "auto_synced";
      break;
    }

    await page.waitForTimeout(500);
  }

  log(label, `Auth/sync result: ${result}`);

  if (result === "auto_synced") {
    log(label, "Auto-sync completed, skipping init flow");
    // Close any dialogs that might be open
    const closeButton = page.getByRole("button", { name: "Close" });
    if (await closeButton.isVisible().catch(() => false)) {
      log(label, "Closing dialog");
      await closeButton.click();
      await page.waitForTimeout(500);
    }
    return;
  }

  if (result === "timeout") {
    // One last check
    if (await isSynced()) {
      log(label, "Already synced (detected after timeout)");
      return;
    }
    throw new Error(`${label}: Neither mode button nor synced indicator appeared`);
  }

  // Init flow - click the mode button
  await modeButton.click();
  log(label, `Init flow - clicked ${modeLabel}`);

  // Wait for Start Syncing button to be enabled and click it
  await startSyncButton.waitFor({ state: "visible", timeout: 5000 });
  await startSyncButton.click();
  log(label, "Clicked Start Syncing");

  // Wait for sync to complete - the "Syncing..." indicator should disappear
  // and the dialog should close
  log(label, "Waiting for sync to complete...");

  const syncingIndicator = page.getByText("Syncing...");

  // Wait for syncing indicator to appear first (confirms sync started)
  try {
    await syncingIndicator.waitFor({ state: "visible", timeout: 5000 });
    log(label, "Sync started, waiting for completion...");
  } catch {
    log(label, "Syncing indicator not seen, sync may have completed quickly");
  }

  // Now wait for sync to complete - dialog closes when done
  await expect(initDialog).toBeHidden({ timeout: 60000 });
  log(label, "Sync completed, dialog closed");

  // Additional wait to ensure sync state is propagated
  await page.waitForTimeout(1000);

  // Ensure any remaining dialogs are closed
  const closeButton = page.getByRole("button", { name: "Close" });
  if (await closeButton.isVisible().catch(() => false)) {
    log(label, "Closing remaining dialog");
    await closeButton.click();
    await page.waitForTimeout(500);
  }
}

test.describe.serial("Sync", () => {
  test.beforeAll(async () => {
    if (shouldStartServer) {
      // Validate binary exists before starting
      if (!existsSync(syncServerBinary)) {
        log("server", `Binary not found at: ${syncServerBinary}`);
        log(
          "server",
          "Build server first: cargo build --release -p diaryx_sync_server",
        );
        throw new Error(
          `Sync server binary not found. Build with: cargo build --release -p diaryx_sync_server`,
        );
      }

      // Create unique temp directory for server data
      tempDataDir = mkdtempSync(path.join(tmpdir(), "diaryx-sync-test-"));
      log("server", `Using temp data dir: ${tempDataDir}`);

      log("server", `Starting server from binary: ${syncServerBinary}`);
      serverProcess = spawn(syncServerBinary, [], {
        cwd: repoRoot,
        stdio: "pipe",
        env: {
          ...process.env,
          DATABASE_PATH: path.join(tempDataDir, "test.db"),
        },
      });

      // Forward server output to test output
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

    // Clean up temp directory
    if (tempDataDir && existsSync(tempDataDir)) {
      log("server", `Cleaning up temp dir: ${tempDataDir}`);
      rmSync(tempDataDir, { recursive: true, force: true });
      tempDataDir = null;
    }
  });

  test("syncs edits between two clients", async ({ browser, browserName }) => {
    test.setTimeout(120000);
    test.skip(browserName === 'webkit', 'WebKit OPFS not fully supported');
    test.skip(!serverAvailable, "Sync server not available");

    log("test", "Creating browser contexts");
    const contextA = await browser.newContext();
    const contextB = await browser.newContext();

    const pageA = await contextA.newPage();
    const pageB = await contextB.newPage();

    // Set up console log forwarding
    setupConsoleLogs(pageA, "clientA");
    setupConsoleLogs(pageB, "clientB");

    log("test", "Navigating to app");
    await pageA.goto("/");
    await pageB.goto("/");

    // Clear all storage for fresh workspace state
    log("clientA", "Clearing browser storage");
    await clearAllBrowserStorage(pageA);
    log("clientB", "Clearing browser storage");
    await clearAllBrowserStorage(pageB);

    // Reload after clearing storage to get fresh minimal workspace
    log("test", "Reloading pages after storage clear");
    await pageA.reload();
    await pageB.reload();

    log("test", "Waiting for app ready");
    await waitForAppReady(pageA);
    await waitForAppReady(pageB);

    const runSuffix = Date.now();
    const testEmail = `sync-test-${runSuffix}@example.com`;
    log("test", `Using test email: ${testEmail}`);

    log("test", "Starting auth for clientA");
    await completeAuthAndInit(pageA, testEmail, /Sync local content/i, "clientA");

    log("test", "Starting auth for clientB");
    await completeAuthAndInit(pageB, testEmail, /Load from server/i, "clientB");

    log("test", "Setting up editors");
    const editorA = new EditorHelper(pageA);
    const editorB = new EditorHelper(pageB);

    await editorA.waitForReady();
    await editorB.waitForReady();
    log("test", "Both editors ready");

    const uniqueText = `Sync test ${Date.now()}`;
    log("clientA", `Inserting unique text: ${uniqueText}`);

    // Focus the editor first
    await editorA.editor.click();
    await pageA.waitForTimeout(500);

    // Insert content programmatically using TipTap's commands
    // This properly triggers TipTap's onChange callback
    const insertResult = await pageA.evaluate((text) => {
      // Get the ProseMirror editor view from the DOM element
      const editorEl = document.querySelector('.ProseMirror') as any;
      if (!editorEl || !editorEl.pmViewDesc?.view) {
        // Fallback: try to find the editor instance another way
        // The Svelte Editor component stores editorRef which has editor
        console.log('[Test] ProseMirror view not found on element, trying fallback');
        return { success: false, error: 'ProseMirror view not found' };
      }

      const view = editorEl.pmViewDesc.view;
      const { state, dispatch } = view;

      // Insert at the end of the document
      const tr = state.tr.insertText('\n' + text, state.doc.content.size);
      dispatch(tr);

      return { success: true };
    }, uniqueText);

    log("clientA", `Insert result: ${JSON.stringify(insertResult)}`);

    // If programmatic insert didn't work, fall back to keyboard
    if (!insertResult.success) {
      log("clientA", "Falling back to keyboard input");
      await editorA.editor.press("End"); // Move to end
      await pageA.keyboard.press("Enter");
      await pageA.keyboard.type(uniqueText, { delay: 30 });
    }

    // Verify text was inserted
    await pageA.waitForTimeout(500);
    const editorText = await editorA.editor.textContent();
    log("clientA", `Editor content after insert: "${editorText?.slice(0, 100)}..."`);
    await expect(editorA.editor).toContainText(uniqueText, { timeout: 10000 });
    log("clientA", "Text inserted successfully");

    // Trigger save by dispatching a transaction (marks dirty) and then blur
    log("clientA", "Triggering save...");

    // First mark the entry as dirty by triggering the onchange callback
    await pageA.evaluate(() => {
      // Dispatch input event to trigger React/Svelte change handlers
      const editorEl = document.querySelector('.ProseMirror');
      if (editorEl) {
        editorEl.dispatchEvent(new Event('input', { bubbles: true }));
      }
    });

    await pageA.waitForTimeout(100);

    // Now blur to trigger the onblur save handler
    await pageA.evaluate(() => {
      const editorEl = document.querySelector('.ProseMirror') as HTMLElement;
      if (editorEl) {
        editorEl.blur();
      }
    });

    // Wait for save indicator to show "Saving..." then "Saved"
    log("clientA", "Waiting for save to complete...");

    // The "Saved" button appears when save completes
    const savedButton = pageA.getByRole("button", { name: "Saved" });
    try {
      await savedButton.waitFor({ state: "visible", timeout: 10000 });
      log("clientA", "Save completed (Saved indicator visible)");
    } catch {
      log("clientA", "Save indicator not seen, save may not have triggered");
      // Try one more time - use keyboard shortcut to save
      log("clientA", "Trying Cmd+S to force save");
      await pageA.keyboard.press("Meta+s");
      await pageA.waitForTimeout(2000);
    }

    // Wait for sync to propagate
    log("clientA", "Waiting for sync to propagate to server...");
    await pageA.waitForTimeout(5000);

    // Click on editorB to ensure it gets focus and any pending updates are applied
    log("clientB", "Focusing editor to receive updates");
    await editorB.focus();

    log("clientB", "Waiting for text to sync");
    await expect(editorB.editor).toContainText(uniqueText, { timeout: 30000 });
    log("test", "Text synced successfully!");

    log("test", "Closing contexts");
    await contextA.close();
    await contextB.close();
  });
});
