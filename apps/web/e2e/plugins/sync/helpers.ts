/**
 * Shared helpers for sync e2e tests.
 *
 * This module extracts the common setup, teardown, and bridge helpers so that
 * individual spec files stay focused on the scenario they test.
 */

import { test, expect, waitForAppReady } from "../../fixtures";
import { execFileSync, spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { access, readFile, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";

// ---------------------------------------------------------------------------
// Environment / paths
// ---------------------------------------------------------------------------

export const WEB_HOST = process.env.PW_WEB_HOST ?? "127.0.0.1";
export const WEB_PORT = process.env.PW_WEB_PORT ?? "5174";
export const APP_BASE_URL = process.env.PW_BASE_URL ?? `http://localhost:${WEB_PORT}`;
export const SYNC_SERVER_URL = process.env.SYNC_SERVER_URL
  ?? `http://${process.env.SYNC_SERVER_HOST ?? "127.0.0.1"}:${process.env.SYNC_SERVER_PORT ?? "3030"}`;
const SHOULD_START_SYNC_SERVER = process.env.SYNC_E2E_START_SERVER !== "0";

const REPO_ROOT = path.resolve(process.cwd(), "..", "..");
const DEFAULT_SYNC_PLUGIN_DIR = path.resolve(REPO_ROOT, "..", "plugin-sync");
const DEFAULT_SYNC_PLUGIN_WASM = path.join(
  DEFAULT_SYNC_PLUGIN_DIR,
  "target",
  "wasm32-unknown-unknown",
  "debug",
  "diaryx_sync_extism.wasm",
);

// ---------------------------------------------------------------------------
// Module-level state (sync server & plugin cache)
// ---------------------------------------------------------------------------

let syncPluginBase64Promise: Promise<string> | null = null;
let spawnedSyncServer: ChildProcessWithoutNullStreams | null = null;
let spawnedSyncServerDbPath: string | null = null;

// ---------------------------------------------------------------------------
// Small utilities
// ---------------------------------------------------------------------------

export function escapeRegex(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

async function fileExists(filePath: string): Promise<boolean> {
  try {
    await access(filePath);
    return true;
  } catch {
    return false;
  }
}

async function fsMkdirTemp(prefix: string): Promise<string> {
  const { mkdtemp } = await import("node:fs/promises");
  return mkdtemp(path.join(os.tmpdir(), prefix));
}

// ---------------------------------------------------------------------------
// Sync server lifecycle
// ---------------------------------------------------------------------------

export async function waitForHttpOk(url: string, timeoutMs: number): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  let lastError: unknown = null;

  while (Date.now() < deadline) {
    try {
      const response = await fetch(url);
      if (response.ok) {
        return;
      }
      lastError = new Error(`HTTP ${response.status} from ${url}`);
    } catch (error) {
      lastError = error;
    }

    await new Promise((resolve) => setTimeout(resolve, 500));
  }

  throw new Error(
    `Timed out waiting for ${url}: ${lastError instanceof Error ? lastError.message : String(lastError)}`,
  );
}

export async function ensureSyncServer(): Promise<void> {
  await waitForHttpOk(`${SYNC_SERVER_URL}/health`, 1000).catch(() => undefined);

  try {
    await waitForHttpOk(`${SYNC_SERVER_URL}/health`, 1000);
    return;
  } catch {
    // Fall through to optional auto-start.
  }

  if (!SHOULD_START_SYNC_SERVER) {
    throw new Error(
      `Sync server is not reachable at ${SYNC_SERVER_URL}. Start it manually or unset SYNC_E2E_START_SERVER=0.`,
    );
  }

  if (spawnedSyncServer) {
    await waitForHttpOk(`${SYNC_SERVER_URL}/health`, 60000);
    return;
  }

  const parsed = new URL(SYNC_SERVER_URL);
  const syncHost = parsed.hostname;
  const syncPort = parsed.port || (parsed.protocol === "https:" ? "443" : "80");
  const corsOrigins = [APP_BASE_URL, `http://${WEB_HOST}:${WEB_PORT}`].join(",");
  const tempDir = await fsMkdirTemp("diaryx-sync-e2e-");
  spawnedSyncServerDbPath = path.join(tempDir, "sync-e2e.sqlite3");

  spawnedSyncServer = spawn(
    "cargo",
    ["run", "-p", "diaryx_sync_server"],
    {
      cwd: REPO_ROOT,
      env: {
        ...process.env,
        HOST: syncHost,
        PORT: syncPort,
        APP_BASE_URL,
        CORS_ORIGINS: corsOrigins,
        DATABASE_PATH: spawnedSyncServerDbPath,
      },
      stdio: ["ignore", "pipe", "pipe"],
    },
  );

  spawnedSyncServer.stdout.on("data", (chunk) => {
    process.stdout.write(`[sync-e2e-server] ${chunk}`);
  });
  spawnedSyncServer.stderr.on("data", (chunk) => {
    process.stderr.write(`[sync-e2e-server] ${chunk}`);
  });

  spawnedSyncServer.once("exit", (code, signal) => {
    if (code !== 0 && signal !== "SIGTERM") {
      process.stderr.write(
        `[sync-e2e-server] exited unexpectedly (code=${String(code)} signal=${String(signal)})\n`,
      );
    }
  });

  await waitForHttpOk(`${SYNC_SERVER_URL}/health`, 60000);
}

export async function stopSpawnedSyncServer(): Promise<void> {
  // Only stop the server if this process spawned it.  When multiple Playwright
  // workers import this module, each gets its own copy of spawnedSyncServer.
  // The first worker that spawned the server is the only one with a non-null
  // reference, so only that worker can (and should) kill it.  However, in a
  // multi-file setup the first worker to finish would kill the shared server
  // while other workers are still using it.
  //
  // To avoid this, we intentionally skip cleanup here and let the child
  // process be reaped when the parent Playwright process exits.  The temp
  // DB directory is cleaned up by the OS or a subsequent run.
  //
  // For explicit cleanup (e.g. running a single file), set
  // SYNC_E2E_CLEANUP_SERVER=1.
  if (process.env.SYNC_E2E_CLEANUP_SERVER !== "1") {
    return;
  }

  const child = spawnedSyncServer;
  spawnedSyncServer = null;

  if (child && !child.killed) {
    child.kill("SIGTERM");
    await new Promise<void>((resolve) => {
      child.once("exit", () => resolve());
      setTimeout(() => {
        if (!child.killed) {
          child.kill("SIGKILL");
        }
      }, 5000);
    }).catch(() => undefined);
  }

  const dbPath = spawnedSyncServerDbPath;
  spawnedSyncServerDbPath = null;
  if (dbPath) {
    await rm(path.dirname(dbPath), { recursive: true, force: true });
  }
}

// ---------------------------------------------------------------------------
// Sync plugin WASM
// ---------------------------------------------------------------------------

export async function ensureSyncPluginBase64(): Promise<string> {
  if (syncPluginBase64Promise) {
    return syncPluginBase64Promise;
  }

  syncPluginBase64Promise = (async () => {
    const explicitPath = process.env.DIARYX_SYNC_PLUGIN_WASM?.trim();
    const wasmPath = explicitPath || DEFAULT_SYNC_PLUGIN_WASM;

    if (!explicitPath) {
      if (!(await fileExists(DEFAULT_SYNC_PLUGIN_DIR))) {
        throw new Error(
          `Expected sibling plugin repo at ${DEFAULT_SYNC_PLUGIN_DIR}. Set DIARYX_SYNC_PLUGIN_WASM to override the artifact path.`,
        );
      }

      execFileSync(
        "cargo",
        ["build", "--target", "wasm32-unknown-unknown"],
        {
          cwd: DEFAULT_SYNC_PLUGIN_DIR,
          stdio: "inherit",
        },
      );
    }

    if (!(await fileExists(wasmPath))) {
      throw new Error(`Sync plugin wasm not found at ${wasmPath}`);
    }

    const bytes = await readFile(wasmPath);
    return Buffer.from(bytes).toString("base64");
  })();

  return syncPluginBase64Promise;
}

// ---------------------------------------------------------------------------
// Plugin installation helpers
// ---------------------------------------------------------------------------

export async function installSyncPlugin(
  page: import("@playwright/test").Page,
  wasmBase64: string,
): Promise<void> {
  const acceptDialog = page.waitForEvent("dialog").then((dialog) => dialog.accept()).catch(() => undefined);

  await page.evaluate(async (encodedWasm) => {
    const { installLocalPlugin } = await import("/src/lib/plugins/pluginInstallService");
    const binary = atob(encodedWasm);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i += 1) {
      bytes[i] = binary.charCodeAt(i);
    }
    const buffer = bytes.buffer.slice(
      bytes.byteOffset,
      bytes.byteOffset + bytes.byteLength,
    );
    await installLocalPlugin(buffer, "diaryx-sync-e2e");
  }, wasmBase64);

  await acceptDialog;
  await page.reload();
  await waitForAppReady(page, 45000);
}

export async function installSyncPluginInCurrentWorkspace(
  page: import("@playwright/test").Page,
  wasmBase64: string,
): Promise<void> {
  const acceptDialog = page.waitForEvent("dialog").then((dialog) => dialog.accept()).catch(() => undefined);

  await page.evaluate(async (encodedWasm) => {
    const bridge = (globalThis as any).__diaryx_e2e;
    if (!bridge) throw new Error("Diaryx E2E bridge is not available");
    await bridge.installPluginInCurrentWorkspace(encodedWasm);
  }, wasmBase64);

  await acceptDialog;
  await page.reload();
  await waitForAppReady(page, 45000);
  await waitForE2EBridge(page);
  await page.evaluate(() => (globalThis as any).__diaryx_e2e.setAutoAllowPermissions(true));
}

// ---------------------------------------------------------------------------
// Auth helpers
// ---------------------------------------------------------------------------

export async function signInWithDevMagicLink(
  page: import("@playwright/test").Page,
  email: string,
): Promise<void> {
  await page.getByRole("button", { name: /^sign in$/i }).first().click();
  await page.getByRole("button", { name: /advanced/i }).click();
  await page.getByRole("textbox", { name: /^email$/i }).fill(email);
  await page.getByLabel("Server URL").fill(SYNC_SERVER_URL);
  await page.getByRole("button", { name: /send sign-in link/i }).click();
  await page.getByRole("link", { name: /click here to verify/i }).click();

  let authPollCount = 0;
  await expect
    .poll(async () => {
      const hasToken = await page.evaluate(() => !!localStorage.getItem("diaryx_auth_token"));
      if (!hasToken && authPollCount++ % 5 === 0) {
        process.stderr.write(`[sync-e2e] auth_token poll #${authPollCount}: no token yet\n`);
      }
      if (hasToken && authPollCount < 100) {
        process.stderr.write(`[sync-e2e] auth_token poll: token found after ${authPollCount} polls\n`);
        authPollCount = 100;
      }
      return hasToken;
    },
      { timeout: 45000 },
    )
    .toBe(true);

  let providerPollCount = 0;
  await expect
    .poll(async () => {
      const result = await page.evaluate(async () => {
        const { getBackend, createApi } = await import("/src/lib/backend");
        const backend = await getBackend();
        const api = createApi(backend);

        try {
          const res = await api.executePluginCommand("diaryx.sync", "GetProviderStatus", {});
          return res;
        } catch (e) {
          return { error: String(e) };
        }
      });
      const ready = (result as { ready?: boolean })?.ready ?? false;
      if (!ready && providerPollCount++ % 5 === 0) {
        process.stderr.write(`[sync-e2e] GetProviderStatus poll #${providerPollCount}: ${JSON.stringify(result)}\n`);
      }
      if (ready && providerPollCount < 100) {
        process.stderr.write(`[sync-e2e] GetProviderStatus: ready after ${providerPollCount} polls\n`);
        providerPollCount = 100;
      }
      return ready;
    },
      { timeout: 45000 },
    )
    .toBe(true);

  await expect(page.getByText(email).first()).toBeVisible({ timeout: 15000 });
  await page.locator("main").click({ position: { x: 40, y: 40 } });
}

export async function removeCurrentDeviceFromAccount(
  page: import("@playwright/test").Page,
): Promise<string> {
  return page.evaluate(async () => {
    const token = localStorage.getItem("diaryx_auth_token");
    const serverUrl = localStorage.getItem("diaryx_sync_server_url");
    if (!token || !serverUrl) {
      throw new Error("Expected authenticated sync session before removing device");
    }

    const [{ getDeviceId }, { proxyFetch }] = await Promise.all([
      import("/src/lib/device/deviceId"),
      import("/src/lib/backend/proxyFetch"),
    ]);

    const deviceId = getDeviceId();
    const response = await proxyFetch(`${serverUrl}/auth/devices/${deviceId}`, {
      method: "DELETE",
      headers: {
        Authorization: `Bearer ${token}`,
      },
    });

    if (!response.ok) {
      throw new Error(`Failed to delete current device: HTTP ${response.status}`);
    }

    return deviceId;
  });
}

// ---------------------------------------------------------------------------
// Permission prompt helpers
// ---------------------------------------------------------------------------

export async function allowPermissionPrompts(
  page: import("@playwright/test").Page,
  idleMs: number = 1000,
  timeoutMs: number = 60000,
): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  let lastSeenAt = Date.now();

  while (Date.now() < deadline) {
    const allowButton = page.getByRole("button", { name: /^allow$/i }).first();
    const visible = await allowButton.isVisible().catch(() => false);

    if (visible) {
      await allowButton.click({ force: true });
      lastSeenAt = Date.now();
      continue;
    }

    if (Date.now() - lastSeenAt >= idleMs) {
      return;
    }

    await page.waitForTimeout(250);
  }
}

// ---------------------------------------------------------------------------
// Workspace helpers
// ---------------------------------------------------------------------------

export async function currentWorkspaceName(
  page: import("@playwright/test").Page,
): Promise<string | null> {
  return page.evaluate(() => {
    const rawRegistry = localStorage.getItem("diaryx_local_workspaces");
    const currentId = localStorage.getItem("diaryx_current_workspace");
    if (!rawRegistry || !currentId) {
      return localStorage.getItem("diaryx-workspace-name");
    }

    const registry = JSON.parse(rawRegistry) as Array<{ id?: string; name?: string }>;
    return registry.find((entry) => entry.id === currentId)?.name ?? null;
  });
}

export async function currentWorkspaceProviderLink(
  page: import("@playwright/test").Page,
): Promise<{ pluginId: string; remoteWorkspaceId: string } | null> {
  return page.evaluate(() => {
    const rawRegistry = localStorage.getItem("diaryx_local_workspaces");
    const currentId = localStorage.getItem("diaryx_current_workspace");
    if (!rawRegistry || !currentId) return null;

    const registry = JSON.parse(rawRegistry) as Array<{
      id?: string;
      pluginMetadata?: Record<string, Record<string, unknown>>;
    }>;
    const workspace = registry.find((entry) => entry.id === currentId);
    const pluginMetadata = workspace?.pluginMetadata ?? {};

    for (const [pluginId, metadata] of Object.entries(pluginMetadata)) {
      const effectivePluginId = pluginId === "sync" ? "diaryx.sync" : pluginId;
      const remoteWorkspaceId =
        typeof metadata?.remoteWorkspaceId === "string" && metadata.remoteWorkspaceId.trim().length > 0
          ? metadata.remoteWorkspaceId
          : typeof metadata?.serverId === "string" && metadata.serverId.trim().length > 0
            ? metadata.serverId
            : null;

      if (remoteWorkspaceId) {
        return {
          pluginId: effectivePluginId,
          remoteWorkspaceId,
        };
      }
    }

    return null;
  });
}

export async function createSyncedWorkspaceViaUi(
  page: import("@playwright/test").Page,
  workspaceName: string,
): Promise<{ remoteId: string; localId: string }> {
  const selectorName = await currentWorkspaceName(page);
  if (!selectorName) {
    throw new Error("No current workspace name available for selector");
  }

  await page.getByRole("button", { name: new RegExp(escapeRegex(selectorName)) }).last().click();
  await page.getByRole("button", { name: /new workspace/i }).click();

  const dialog = page.getByRole("dialog").last();
  await expect(dialog).toBeVisible({ timeout: 15000 });
  await dialog.getByLabel("Workspace Name").fill(workspaceName);
  await dialog.locator("select").selectOption("diaryx.sync");
  await expect(dialog.getByRole("button", { name: /create & sync/i })).toBeEnabled({ timeout: 45000 });
  await dialog.getByRole("button", { name: /create & sync/i }).click();
  await allowPermissionPrompts(page);

  await expect(dialog).not.toBeVisible({ timeout: 60000 });
  await expect
    .poll(async () => await currentWorkspaceName(page), { timeout: 45000 })
    .toBe(workspaceName);

  await waitForSyncSession(page);

  await expect
    .poll(async () => await currentWorkspaceProviderLink(page), { timeout: 30000 })
    .toEqual({
      pluginId: "diaryx.sync",
      remoteWorkspaceId: expect.any(String),
    });

  const providerLink = await currentWorkspaceProviderLink(page);
  if (!providerLink) {
    throw new Error("Expected synced workspace provider link to be persisted");
  }

  const localId = await page.evaluate(() => localStorage.getItem("diaryx_current_workspace"));
  if (!localId) {
    throw new Error("Expected current local workspace ID after sync workspace creation");
  }

  return {
    remoteId: providerLink.remoteWorkspaceId,
    localId,
  };
}

export async function downloadRemoteWorkspaceViaUi(
  page: import("@playwright/test").Page,
  remoteWorkspaceName: string,
  remoteWorkspaceId?: string,
): Promise<void> {
  const selectorName = await currentWorkspaceName(page);
  if (!selectorName) {
    throw new Error("No current workspace name available for selector");
  }
  process.stderr.write(`[sync-e2e] downloadRemoteWorkspace: clicking selector for "${selectorName}"\n`);

  await page.getByRole("button", { name: new RegExp(escapeRegex(selectorName)) }).last().click();
  const newWorkspaceButton = page.getByRole("button", { name: /^new workspace$/i });
  await expect(newWorkspaceButton).toBeVisible({ timeout: 15000 });

  const moreOnButton = page.getByRole("button", { name: /more on/i }).first();
  const hasRemoteShortcut = await moreOnButton.isVisible().catch(() => false);

  if (hasRemoteShortcut) {
    process.stderr.write(`[sync-e2e] downloadRemoteWorkspace: using selector remote shortcut\n`);
    await moreOnButton.click();
    process.stderr.write(`[sync-e2e] downloadRemoteWorkspace: clicking remote workspace "${remoteWorkspaceName}"\n`);
    const remoteWorkspaceButton = page.getByRole("button", {
      name: new RegExp(escapeRegex(remoteWorkspaceName), "i"),
    }).first();
    await expect(remoteWorkspaceButton).toBeVisible({ timeout: 15000 });
    await remoteWorkspaceButton.click();
  } else if (remoteWorkspaceId) {
    process.stderr.write(
      `[sync-e2e] downloadRemoteWorkspace: using direct provider download for "${remoteWorkspaceName}" (${remoteWorkspaceId})\n`,
    );
    await Promise.all([
      page.evaluate(async ({ remoteId, name }) => {
        const { downloadWorkspace } = await import("/src/lib/sync/workspaceProviderService");
        await downloadWorkspace("diaryx.sync", { remoteId, name, link: true });
      }, { remoteId: remoteWorkspaceId, name: remoteWorkspaceName }),
      allowPermissionPrompts(page, 1000, 15000),
    ]);
  } else {
    process.stderr.write(`[sync-e2e] downloadRemoteWorkspace: falling back to add-workspace dialog\n`);
    await newWorkspaceButton.click();

    const dialog = page.getByRole("dialog");
    await expect(dialog.getByRole("heading", { name: /add workspace/i })).toBeVisible({ timeout: 15000 });

    const downloadFromCloudButton = dialog.getByRole("button", { name: /download from cloud/i });
    await expect(downloadFromCloudButton).toBeVisible({ timeout: 15000 });
    await downloadFromCloudButton.click();

    const remoteWorkspaceButton = dialog.getByRole("button", {
      name: new RegExp(escapeRegex(remoteWorkspaceName), "i"),
    }).first();
    const cloudLoadingIndicator = dialog.getByText(/loading cloud workspaces/i);
    for (let attempt = 0; attempt < 30; attempt += 1) {
      if (await remoteWorkspaceButton.isVisible().catch(() => false)) {
        break;
      }

      const loading = await cloudLoadingIndicator.isVisible().catch(() => false);
      if (!loading) {
        process.stderr.write(
          `[sync-e2e] downloadRemoteWorkspace: refreshing cloud workspace list (attempt ${attempt + 1})\n`,
        );
        await downloadFromCloudButton.click();
      }

      await page.waitForTimeout(1000);
    }

    await expect(remoteWorkspaceButton).toBeVisible({ timeout: 5000 });
    await remoteWorkspaceButton.click();

    const downloadWorkspaceButton = dialog.getByRole("button", { name: /^download workspace$/i });
    await expect(downloadWorkspaceButton).toBeEnabled({ timeout: 15000 });
    await downloadWorkspaceButton.click();
  }

  process.stderr.write(`[sync-e2e] downloadRemoteWorkspace: allowing permission prompts\n`);
  await allowPermissionPrompts(page);
  process.stderr.write(`[sync-e2e] downloadRemoteWorkspace: starting poll\n`);

  let pollCount = 0;
  await expect
    .poll(async () => {
      const name = await currentWorkspaceName(page);
      if (pollCount++ % 10 === 0) {
        process.stderr.write(`[sync-e2e] downloadRemoteWorkspace poll #${pollCount}: name=${JSON.stringify(name)} expected=${JSON.stringify(remoteWorkspaceName)}\n`);
      }
      return name;
    }, { timeout: 60000 })
    .toBe(remoteWorkspaceName);
}

// ---------------------------------------------------------------------------
// Sync status helpers
// ---------------------------------------------------------------------------

export async function waitForSyncSession(
  page: import("@playwright/test").Page,
): Promise<void> {
  await expect
    .poll(async () =>
      page.evaluate(() => {
        const bridge = (globalThis as {
          __diaryx_e2e?: {
            getSyncStatus: () => Promise<string | null>;
          };
        }).__diaryx_e2e;

        return (async () => {
          try {
            const { getBackend, createApi } = await import("/src/lib/backend");
            const backend = await getBackend();
            const api = createApi(backend);
            const status = await api.executePluginCommand("diaryx.sync", "GetSyncStatus", {});

            if (
              status
              && typeof status === "object"
              && "state" in status
              && typeof (status as { state?: unknown }).state === "string"
            ) {
              return (status as { state: string }).state;
            }
          } catch {
            // Fall back to the UI bridge while the plugin/backend is still booting.
          }

          if (!bridge) {
            return null;
          }

          return bridge.getSyncStatus();
        })();
      }),
      { timeout: 45000 },
    )
    .toMatch(/^(syncing|synced)$/);
}

export async function waitForE2EBridge(
  page: import("@playwright/test").Page,
  timeoutMs: number = 30000,
): Promise<void> {
  await expect
    .poll(async () => {
      return await page.evaluate(() => !!(globalThis as any).__diaryx_e2e);
    }, { timeout: timeoutMs })
    .toBe(true);
}

// ---------------------------------------------------------------------------
// Entry CRUD helpers
// ---------------------------------------------------------------------------

export async function createEntryWithMarker(
  page: import("@playwright/test").Page,
  stem: string,
  marker: string,
): Promise<string> {
  const createPromise = page.evaluate(({ entryStem, entryMarker }) => {
    const bridge = (globalThis as {
      __diaryx_e2e?: {
        createEntryWithMarker: (stem: string, marker: string) => Promise<string>;
      };
    }).__diaryx_e2e;

    if (!bridge) {
      throw new Error("Diaryx E2E bridge is not available");
    }

    return bridge.createEntryWithMarker(entryStem, entryMarker);
  }, {
    entryStem: stem,
    entryMarker: marker,
  });

  return await Promise.all([
    createPromise,
    allowPermissionPrompts(page, 1000, 15000),
  ]).then(([result]) => result);
}

export async function appendMarkerToEntry(
  page: import("@playwright/test").Page,
  entryPath: string,
  marker: string,
): Promise<void> {
  const updatePromise = page.evaluate(({ pathToUpdate, entryMarker }) => {
    const bridge = (globalThis as {
      __diaryx_e2e?: {
        appendMarkerToEntry: (path: string, marker: string) => Promise<void>;
      };
    }).__diaryx_e2e;

    if (!bridge) {
      throw new Error("Diaryx E2E bridge is not available");
    }

    return bridge.appendMarkerToEntry(pathToUpdate, entryMarker);
  }, {
    pathToUpdate: entryPath,
    entryMarker: marker,
  });

  await Promise.all([
    updatePromise,
    allowPermissionPrompts(page, 1000, 15000),
  ]);
}

export async function createIndexEntry(
  page: import("@playwright/test").Page,
  stem: string,
): Promise<string> {
  const createPromise = page.evaluate((entryStem) => {
    const bridge = (globalThis as {
      __diaryx_e2e?: {
        createIndexEntry: (stem: string) => Promise<string>;
      };
    }).__diaryx_e2e;

    if (!bridge) {
      throw new Error("Diaryx E2E bridge is not available");
    }

    return bridge.createIndexEntry(entryStem);
  }, stem);

  return await Promise.all([
    createPromise,
    allowPermissionPrompts(page, 1000, 15000),
  ]).then(([result]) => result);
}

export async function renameEntry(
  page: import("@playwright/test").Page,
  entryPath: string,
  newFilename: string,
): Promise<string> {
  const renamePromise = page.evaluate(({ pathToRename, filename }) => {
    const bridge = (globalThis as {
      __diaryx_e2e?: {
        renameEntry: (path: string, newFilename: string) => Promise<string>;
      };
    }).__diaryx_e2e;

    if (!bridge) {
      throw new Error("Diaryx E2E bridge is not available");
    }

    return bridge.renameEntry(pathToRename, filename);
  }, {
    pathToRename: entryPath,
    filename: newFilename,
  });

  return await Promise.all([
    renamePromise,
    allowPermissionPrompts(page, 1000, 15000),
  ]).then(([result]) => result);
}

export async function moveEntryToParent(
  page: import("@playwright/test").Page,
  entryPath: string,
  parentPath: string,
): Promise<string> {
  const movePromise = page.evaluate(({ pathToMove, destinationParentPath }) => {
    const bridge = (globalThis as {
      __diaryx_e2e?: {
        moveEntryToParent: (path: string, parentPath: string) => Promise<string>;
      };
    }).__diaryx_e2e;

    if (!bridge) {
      throw new Error("Diaryx E2E bridge is not available");
    }

    return bridge.moveEntryToParent(pathToMove, destinationParentPath);
  }, {
    pathToMove: entryPath,
    destinationParentPath: parentPath,
  });

  return await Promise.all([
    movePromise,
    allowPermissionPrompts(page, 1000, 15000),
  ]).then(([result]) => result);
}

export async function deleteEntry(
  page: import("@playwright/test").Page,
  entryPath: string,
): Promise<boolean> {
  const deletePromise = page.evaluate((pathToDelete) => {
    const bridge = (globalThis as {
      __diaryx_e2e?: {
        deleteEntry: (path: string) => Promise<boolean>;
      };
    }).__diaryx_e2e;

    if (!bridge) {
      throw new Error("Diaryx E2E bridge is not available");
    }

    return bridge.deleteEntry(pathToDelete);
  }, entryPath);

  return await Promise.all([
    deletePromise,
    allowPermissionPrompts(page, 1000, 15000),
  ]).then(([result]) => result);
}

// ---------------------------------------------------------------------------
// Frontmatter helpers
// ---------------------------------------------------------------------------

export async function setFrontmatterProperty(
  page: import("@playwright/test").Page,
  entryPath: string,
  key: string,
  value: unknown,
): Promise<string | null> {
  const updatePromise = page.evaluate(({ pathToUpdate, propertyKey, propertyValue }) => {
    const bridge = (globalThis as {
      __diaryx_e2e?: {
        setFrontmatterProperty: (
          path: string,
          key: string,
          value: unknown,
        ) => Promise<string | null>;
      };
    }).__diaryx_e2e;

    if (!bridge) {
      throw new Error("Diaryx E2E bridge is not available");
    }

    return bridge.setFrontmatterProperty(pathToUpdate, propertyKey, propertyValue);
  }, {
    pathToUpdate: entryPath,
    propertyKey: key,
    propertyValue: value,
  });

  return await Promise.all([
    updatePromise,
    allowPermissionPrompts(page, 1000, 15000),
  ]).then(([result]) => result);
}

export async function readFrontmatter(
  page: import("@playwright/test").Page,
  entryPath: string,
): Promise<Record<string, unknown> | null> {
  return page.evaluate((pathToRead) => {
    const bridge = (globalThis as {
      __diaryx_e2e?: {
        readFrontmatter: (path: string) => Promise<Record<string, unknown> | null>;
      };
    }).__diaryx_e2e;

    if (!bridge) {
      throw new Error("Diaryx E2E bridge is not available");
    }

    return bridge.readFrontmatter(pathToRead);
  }, entryPath);
}

function normalizeJsonValue(value: unknown): unknown {
  if (Array.isArray(value)) {
    return value.map((item) => normalizeJsonValue(item));
  }

  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>)
        .sort(([left], [right]) => left.localeCompare(right))
        .map(([key, entryValue]) => [key, normalizeJsonValue(entryValue)]),
    );
  }

  return value;
}

export async function readFrontmatterProperty(
  page: import("@playwright/test").Page,
  entryPath: string,
  key: string,
): Promise<unknown> {
  const frontmatter = await readFrontmatter(page, entryPath);
  return normalizeJsonValue(frontmatter?.[key] ?? null);
}

export async function expectFrontmatterProperty(
  page: import("@playwright/test").Page,
  entryPath: string,
  key: string,
  expected: unknown,
  timeoutMs: number = 30000,
): Promise<void> {
  await expect
    .poll(async () => await readFrontmatterProperty(page, entryPath, key), {
      timeout: timeoutMs,
    })
    .toEqual(normalizeJsonValue(expected));
}

// ---------------------------------------------------------------------------
// Entry read helpers
// ---------------------------------------------------------------------------

export async function readEntryBody(
  page: import("@playwright/test").Page,
  entryPath: string,
): Promise<string | null> {
  return page.evaluate((pathToRead) => {
    const bridge = (globalThis as {
      __diaryx_e2e?: {
        readEntryBody: (path: string) => Promise<string | null>;
      };
    }).__diaryx_e2e;

    if (!bridge) {
      throw new Error("Diaryx E2E bridge is not available");
    }

    return bridge.readEntryBody(pathToRead);
  }, entryPath);
}

export async function entryExists(
  page: import("@playwright/test").Page,
  entryPath: string,
): Promise<boolean> {
  return page.evaluate((pathToCheck) => {
    const bridge = (globalThis as {
      __diaryx_e2e?: {
        entryExists: (path: string) => Promise<boolean>;
      };
    }).__diaryx_e2e;

    if (!bridge) {
      throw new Error("Diaryx E2E bridge is not available");
    }

    return bridge.entryExists(pathToCheck);
  }, entryPath);
}

export async function rootEntryPath(
  page: import("@playwright/test").Page,
): Promise<string | null> {
  return page.evaluate(() => {
    const bridge = (globalThis as {
      __diaryx_e2e?: {
        getRootEntryPath: () => string | null;
      };
    }).__diaryx_e2e;

    if (!bridge) {
      throw new Error("Diaryx E2E bridge is not available");
    }

    return bridge.getRootEntryPath();
  });
}

// ---------------------------------------------------------------------------
// Sync-specific helpers
// ---------------------------------------------------------------------------

export async function openEntryForSync(
  page: import("@playwright/test").Page,
  entryPath: string,
): Promise<void> {
  const openPromise = page.evaluate((pathToOpen) => {
    const bridge = (globalThis as {
      __diaryx_e2e?: {
        openEntryForSync: (path: string) => Promise<void>;
      };
    }).__diaryx_e2e;

    if (!bridge) {
      throw new Error("Diaryx E2E bridge is not available");
    }

    return bridge.openEntryForSync(pathToOpen);
  }, entryPath);

  await Promise.all([
    openPromise,
    allowPermissionPrompts(page, 1000, 15000),
  ]);
}

export async function listSyncedFiles(
  page: import("@playwright/test").Page,
): Promise<string[]> {
  return page.evaluate(() => {
    const bridge = (globalThis as {
      __diaryx_e2e?: {
        listSyncedFiles: () => Promise<string[]>;
      };
    }).__diaryx_e2e;

    if (!bridge) {
      throw new Error("Diaryx E2E bridge is not available");
    }

    return bridge.listSyncedFiles();
  });
}

export async function uploadWorkspaceSnapshot(
  page: import("@playwright/test").Page,
  remoteId: string,
): Promise<void> {
  const uploadPromise = page.evaluate(async (linkedRemoteId) => {
    const { getBackend, createApi } = await import("/src/lib/backend");
    const backend = await getBackend();
    const api = createApi(backend);
    await api.executePluginCommand("diaryx.sync", "UploadWorkspaceSnapshot", {
      remote_id: linkedRemoteId,
      mode: "replace",
      include_attachments: true,
    });
  }, remoteId);

  await Promise.all([
    uploadPromise,
    allowPermissionPrompts(page, 1000, 15000),
  ]);
}

export async function executePluginCommand(
  page: import("@playwright/test").Page,
  pluginId: string,
  command: string,
  params: Record<string, unknown>,
): Promise<unknown> {
  for (let attempt = 0; attempt < 3; attempt += 1) {
    try {
      return await page.evaluate(async ({ pid, cmd, par }) => {
        const { getBackend, createApi } = await import("/src/lib/backend");
        const backend = await getBackend();
        const api = createApi(backend);
        return await api.executePluginCommand(pid, cmd, par);
      }, { pid: pluginId, cmd: command, par: params });
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      const executionContextLost = message.includes("Execution context was destroyed")
        || message.includes("Cannot find context with specified id");

      if (!executionContextLost || attempt === 2) {
        throw error;
      }

      await page.waitForLoadState("domcontentloaded", { timeout: 45000 }).catch(() => undefined);
      await waitForAppReady(page, 45000);
      await waitForE2EBridge(page, 45000);
    }
  }

  throw new Error(`Failed to execute ${pluginId}:${command}`);
}

// ---------------------------------------------------------------------------
// Attachment helpers
// ---------------------------------------------------------------------------

export async function uploadAttachment(
  page: import("@playwright/test").Page,
  entryPath: string,
  filename: string,
  dataBase64: string,
): Promise<string> {
  const uploadPromise = page.evaluate(async ({ path, name, data }) => {
    const bridge = (globalThis as any).__diaryx_e2e;
    if (!bridge) throw new Error("Diaryx E2E bridge is not available");
    return await bridge.uploadAttachment(path, name, data);
  }, { path: entryPath, name: filename, data: dataBase64 });

  return await Promise.all([
    uploadPromise,
    allowPermissionPrompts(page, 1000, 15000),
  ]).then(([result]) => result);
}

export async function getAttachments(
  page: import("@playwright/test").Page,
  entryPath: string,
): Promise<string[]> {
  return page.evaluate(async (path) => {
    const bridge = (globalThis as any).__diaryx_e2e;
    if (!bridge) throw new Error("Diaryx E2E bridge is not available");
    return await bridge.getAttachments(path);
  }, entryPath);
}

export async function getAttachmentData(
  page: import("@playwright/test").Page,
  entryPath: string,
  attachmentPath: string,
): Promise<number[]> {
  return page.evaluate(async ({ entry, attachment }) => {
    const bridge = (globalThis as any).__diaryx_e2e;
    if (!bridge) throw new Error("Diaryx E2E bridge is not available");
    return await bridge.getAttachmentData(entry, attachment);
  }, { entry: entryPath, attachment: attachmentPath });
}

// ---------------------------------------------------------------------------
// Synced pair setup
// ---------------------------------------------------------------------------

export type SyncedPair = {
  contextA: import("@playwright/test").BrowserContext;
  contextB: import("@playwright/test").BrowserContext;
  pageA: import("@playwright/test").Page;
  pageB: import("@playwright/test").Page;
  email: string;
  remoteId: string;
  localIdA: string;
  rootPathA: string;
  rootPathB: string;
};

/**
 * Sets up two browser contexts signed into the same account, with a synced
 * workspace created on A and downloaded on B.  Both pages have the sync plugin
 * installed, the e2e bridge available, and auto-allow permissions enabled.
 */
export async function setupSyncedPair(
  browser: import("@playwright/test").Browser,
  label: string,
): Promise<SyncedPair> {
  const wasmBase64 = await ensureSyncPluginBase64();
  const runId = `${Date.now()}-${Math.floor(Math.random() * 100000)}`;
  const email = `sync-e2e-${label}-${runId}@example.com`;
  const remoteWorkspaceName = `Sync E2E ${label} ${runId}`;

  const contextA = await browser.newContext();
  const contextB = await browser.newContext();

  await Promise.all([
    contextA.addInitScript(() => {
      localStorage.setItem("diaryx-storage-type", "indexeddb");
      localStorage.setItem("diaryx_sync_enabled", "true");
      localStorage.setItem("diaryx_e2e_skip_onboarding", "1");
      (globalThis as { __diaryx_e2e_disable_auto_file_open?: boolean }).__diaryx_e2e_disable_auto_file_open = true;
    }),
    contextB.addInitScript(() => {
      localStorage.setItem("diaryx-storage-type", "indexeddb");
      localStorage.setItem("diaryx_sync_enabled", "true");
      localStorage.setItem("diaryx_e2e_skip_onboarding", "1");
      (globalThis as { __diaryx_e2e_disable_auto_file_open?: boolean }).__diaryx_e2e_disable_auto_file_open = true;
    }),
  ]);

  const pageA = await contextA.newPage();
  const pageB = await contextB.newPage();
  for (const [tag, page] of [["A", pageA], ["B", pageB]] as const) {
    page.on("console", (message) => {
      const text = message.text();
      if (text.includes("[extism-plugin:") || text.includes("[extism]") || text.includes("[ws:") || text.includes("[e2e:")) {
        process.stderr.write(`[${label}:page${tag}:${message.type()}] ${text}\n`);
      }
    });
  }

  await Promise.all([
    pageA.goto("/", { timeout: 60000 }),
    pageB.goto("/", { timeout: 60000 }),
  ]);
  await Promise.all([
    waitForAppReady(pageA, 45000),
    waitForAppReady(pageB, 45000),
  ]);

  await Promise.all([
    installSyncPlugin(pageA, wasmBase64),
    installSyncPlugin(pageB, wasmBase64),
  ]);

  await Promise.all([
    signInWithDevMagicLink(pageA, email),
    signInWithDevMagicLink(pageB, email),
  ]);
  await Promise.all([
    waitForE2EBridge(pageA),
    waitForE2EBridge(pageB),
  ]);

  await pageA.evaluate(() => (globalThis as any).__diaryx_e2e.setAutoAllowPermissions(true));
  await pageB.evaluate(() => (globalThis as any).__diaryx_e2e.setAutoAllowPermissions(true));

  const linked = await createSyncedWorkspaceViaUi(pageA, remoteWorkspaceName);
  await installSyncPluginInCurrentWorkspace(pageA, wasmBase64);
  await waitForSyncSession(pageA);

  await expect
    .poll(async () => await rootEntryPath(pageA), { timeout: 15000 })
    .not.toBeNull();
  const rootPathA = await rootEntryPath(pageA);
  if (!rootPathA) {
    throw new Error("Expected a root entry path for the synced workspace");
  }

  await uploadWorkspaceSnapshot(pageA, linked.remoteId);
  await downloadRemoteWorkspaceViaUi(pageB, remoteWorkspaceName, linked.remoteId);

  await expect
    .poll(async () => await currentWorkspaceProviderLink(pageB), { timeout: 30000 })
    .toEqual({
      pluginId: "diaryx.sync",
      remoteWorkspaceId: linked.remoteId,
    });

  await installSyncPluginInCurrentWorkspace(pageB, wasmBase64);
  await waitForSyncSession(pageB);

  await expect
    .poll(async () => await rootEntryPath(pageB), { timeout: 15000 })
    .not.toBeNull();
  const rootPathB = await rootEntryPath(pageB);
  if (!rootPathB) {
    throw new Error("Expected the downloaded workspace to expose a root entry path");
  }

  await expect
    .poll(async () => await readEntryBody(pageB, rootPathB), { timeout: 30000 })
    .toContain(remoteWorkspaceName);

  return {
    contextA,
    contextB,
    pageA,
    pageB,
    email,
    remoteId: linked.remoteId,
    localIdA: linked.localId,
    rootPathA,
    rootPathB,
  };
}

/**
 * Sets up a single browser context signed in with the sync plugin installed
 * and a synced workspace created. Useful for single-client tests.
 */
export async function setupSingleSyncClient(
  browser: import("@playwright/test").Browser,
  label: string,
): Promise<{
  context: import("@playwright/test").BrowserContext;
  page: import("@playwright/test").Page;
  remoteId: string;
  localId: string;
  rootPath: string;
  email: string;
  workspaceName: string;
}> {
  const wasmBase64 = await ensureSyncPluginBase64();
  const runId = `${Date.now()}-${Math.floor(Math.random() * 100000)}`;
  const email = `sync-e2e-${label}-${runId}@example.com`;
  const workspaceName = `Sync E2E ${label} ${runId}`;

  const context = await browser.newContext();
  await context.addInitScript(() => {
    localStorage.setItem("diaryx-storage-type", "indexeddb");
    localStorage.setItem("diaryx_sync_enabled", "true");
    localStorage.setItem("diaryx_e2e_skip_onboarding", "1");
    (globalThis as { __diaryx_e2e_disable_auto_file_open?: boolean }).__diaryx_e2e_disable_auto_file_open = true;
  });

  const page = await context.newPage();
  page.on("console", (message) => {
    const text = message.text();
    if (text.includes("[extism-plugin:") || text.includes("[extism]") || text.includes("[ws:")) {
      process.stderr.write(`[${label}:${message.type()}] ${text}\n`);
    }
  });

  await page.goto("/", { timeout: 60000 });
  await waitForAppReady(page, 45000);
  await installSyncPlugin(page, wasmBase64);
  await signInWithDevMagicLink(page, email);
  await waitForE2EBridge(page);
  await page.evaluate(() => (globalThis as any).__diaryx_e2e.setAutoAllowPermissions(true));

  const linked = await createSyncedWorkspaceViaUi(page, workspaceName);
  await installSyncPluginInCurrentWorkspace(page, wasmBase64);
  await waitForSyncSession(page);

  await expect
    .poll(async () => await rootEntryPath(page), { timeout: 15000 })
    .not.toBeNull();
  const rootPath = await rootEntryPath(page);
  if (!rootPath) {
    throw new Error("Expected a root entry path for the synced workspace");
  }

  return { context, page, remoteId: linked.remoteId, localId: linked.localId, rootPath, email, workspaceName };
}

// ---------------------------------------------------------------------------
// Re-exports
// ---------------------------------------------------------------------------

export { test, expect, waitForAppReady };
