import { test, expect, waitForAppReady } from "../fixtures";
import { execFileSync, spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { access, readFile, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";

const WEB_HOST = process.env.PW_WEB_HOST ?? "127.0.0.1";
const WEB_PORT = process.env.PW_WEB_PORT ?? "5174";
const APP_BASE_URL = process.env.PW_BASE_URL ?? `http://localhost:${WEB_PORT}`;
const SYNC_SERVER_URL = process.env.SYNC_SERVER_URL
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

let syncPluginBase64Promise: Promise<string> | null = null;
let spawnedSyncServer: ChildProcessWithoutNullStreams | null = null;
let spawnedSyncServerDbPath: string | null = null;

function escapeRegex(value: string): string {
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

async function waitForHttpOk(url: string, timeoutMs: number): Promise<void> {
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

async function ensureSyncPluginBase64(): Promise<string> {
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

async function ensureSyncServer(): Promise<void> {
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

async function stopSpawnedSyncServer(): Promise<void> {
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

async function fsMkdirTemp(prefix: string): Promise<string> {
  const { mkdtemp } = await import("node:fs/promises");
  return mkdtemp(path.join(os.tmpdir(), prefix));
}

async function installSyncPlugin(page: import("@playwright/test").Page, wasmBase64: string): Promise<void> {
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

async function installSyncPluginInCurrentWorkspace(
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
}

async function signInWithDevMagicLink(
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
        authPollCount = 100; // prevent repeated logging
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

async function currentWorkspaceName(page: import("@playwright/test").Page): Promise<string | null> {
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

async function allowPermissionPrompts(
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

async function withPermissionAllowance<T>(
  page: import("@playwright/test").Page,
  action: Promise<T>,
): Promise<T> {
  let settled = false;
  let result: T | undefined;
  let error: unknown;

  void action.then(
    (value) => {
      settled = true;
      result = value;
    },
    (reason) => {
      settled = true;
      error = reason;
    },
  );

  while (!settled) {
    const allowButton = page.getByRole("button", { name: /^allow$/i }).first();
    const visible = await allowButton.isVisible().catch(() => false);

    if (visible) {
      await allowButton.click({ force: true });
      continue;
    }

    await page.waitForTimeout(250);
  }

  if (error) {
    throw error;
  }

  return result as T;
}

async function createSyncedWorkspaceViaUi(
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

async function appendMarkerToEntry(
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

async function createEntryWithMarker(
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

async function createIndexEntry(
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

async function renameEntry(
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

async function moveEntryToParent(
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

async function setFrontmatterProperty(
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

async function readEntryBody(
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

async function readFrontmatter(
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

async function readFrontmatterProperty(
  page: import("@playwright/test").Page,
  entryPath: string,
  key: string,
): Promise<unknown> {
  const frontmatter = await readFrontmatter(page, entryPath);
  return normalizeJsonValue(frontmatter?.[key] ?? null);
}

async function expectFrontmatterProperty(
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

async function entryExists(
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

async function listSyncedFiles(
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

async function hasSyncedFile(
  page: import("@playwright/test").Page,
  entryPath: string,
): Promise<boolean> {
  const files = await listSyncedFiles(page);
  return files.includes(entryPath);
}

async function openEntryForSync(
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

async function rootEntryPath(
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

async function uploadWorkspaceSnapshot(
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

async function waitForSyncSession(
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

async function waitForE2EBridge(
  page: import("@playwright/test").Page,
  timeoutMs: number = 30000,
): Promise<void> {
  await expect
    .poll(async () => {
      return await page.evaluate(() => !!(globalThis as any).__diaryx_e2e);
    }, { timeout: timeoutMs })
    .toBe(true);
}

async function downloadRemoteWorkspaceViaUi(
  page: import("@playwright/test").Page,
  remoteWorkspaceName: string,
): Promise<void> {
  const selectorName = await currentWorkspaceName(page);
  if (!selectorName) {
    throw new Error("No current workspace name available for selector");
  }
  process.stderr.write(`[sync-e2e] downloadRemoteWorkspace: clicking selector for "${selectorName}"\n`);

  await page.getByRole("button", { name: new RegExp(escapeRegex(selectorName)) }).last().click();
  process.stderr.write(`[sync-e2e] downloadRemoteWorkspace: clicking "more on"\n`);
  await page.getByRole("button", { name: /more on/i }).click();
  process.stderr.write(`[sync-e2e] downloadRemoteWorkspace: clicking remote workspace "${remoteWorkspaceName}"\n`);
  await page.getByRole("button", { name: new RegExp(escapeRegex(remoteWorkspaceName)) }).click();
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

async function currentWorkspaceProviderLink(
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

test.describe("Sync", () => {
  test.describe.configure({ mode: "serial" });

  test.beforeAll(async ({ browserName }) => {
    if (browserName !== "chromium") {
      return;
    }

    await ensureSyncServer();
    await ensureSyncPluginBase64();
  });

  test.afterAll(async ({ browserName }) => {
    if (browserName !== "chromium") {
      return;
    }

    await stopSpawnedSyncServer();
  });

  test("links, bootstraps, and propagates live workspace changes across two browser clients", async ({ browser, browserName }) => {
    test.skip(browserName !== "chromium", "Sync E2E currently runs on Chromium only");
    test.setTimeout(300000);

    const wasmBase64 = await ensureSyncPluginBase64();
    const runId = `${Date.now()}-${Math.floor(Math.random() * 100000)}`;
    const email = `sync-e2e-${runId}@example.com`;
    const remoteWorkspaceName = `Sync E2E ${runId}`;
    const createdEntryStem = `live-propagation-${runId}`;
    const createdEntryMarker = `CREATED_BODY_${runId}`;
    const renamedEntryFilename = `live-propagation-renamed-${runId}.md`;
    const destinationParentStem = `move-destination-${runId}`;
    const descriptionMarker = `DESCRIPTION_${runId}`;
    const editedBodyMarker = `EDITED_BODY_${runId}`;

    const contextA = await browser.newContext();
    const contextB = await browser.newContext();

    await Promise.all([
      contextA.addInitScript(() => {
        localStorage.setItem("diaryx-storage-type", "indexeddb");
        localStorage.setItem("diaryx_sync_enabled", "true");
      }),
      contextB.addInitScript(() => {
        localStorage.setItem("diaryx-storage-type", "indexeddb");
        localStorage.setItem("diaryx_sync_enabled", "true");
      }),
    ]);

    const pageA = await contextA.newPage();
    const pageB = await contextB.newPage();
    pageA.on("console", (message) => {
      const text = message.text();
      if (text.includes("[extism-plugin:") || text.includes("[extism]") || text.includes("[ws:")) {
        process.stderr.write(`[pageA:${message.type()}] ${text}\n`);
      }
    });
    pageB.on("console", (message) => {
      const text = message.text();
      if (text.includes("[extism-plugin:") || text.includes("[extism]") || text.includes("[ws:")) {
        process.stderr.write(`[pageB:${message.type()}] ${text}\n`);
      }
    });

    try {
      await Promise.all([pageA.goto("/"), pageB.goto("/")]);
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

      // Auto-allow all permission prompts so sync plugin host calls proceed
      // without showing banners that block Playwright interactions.
      await pageA.evaluate(() => (globalThis as any).__diaryx_e2e.setAutoAllowPermissions(true));
      await pageB.evaluate(() => (globalThis as any).__diaryx_e2e.setAutoAllowPermissions(true));

      const linked = await createSyncedWorkspaceViaUi(pageA, remoteWorkspaceName);

      // Re-install sync plugin in the newly created workspace (workspace switch
      // unloads plugins from the previous workspace).
      await installSyncPluginInCurrentWorkspace(pageA, wasmBase64);

      await expect
        .poll(async () => await rootEntryPath(pageA), { timeout: 15000 })
        .not.toBeNull();
      const sharedRootPath = await rootEntryPath(pageA);
      if (!sharedRootPath) {
        throw new Error("Expected a root entry path for the synced workspace");
      }

      await uploadWorkspaceSnapshot(pageA, linked.remoteId);

      await downloadRemoteWorkspaceViaUi(pageB, remoteWorkspaceName);

      // Wait for the provider link to be set before reinstalling the plugin.
      // downloadWorkspace sets currentWorkspaceId early (making the name poll
      // pass) but the provider link is only set after snapshot extraction.
      await expect
        .poll(async () => await currentWorkspaceProviderLink(pageB), { timeout: 30000 })
        .toEqual({
          pluginId: "diaryx.sync",
          remoteWorkspaceId: linked.remoteId,
        });

      // Re-install sync plugin in the downloaded workspace so it picks up the
      // workspace_id from the provider link during init.
      await installSyncPluginInCurrentWorkspace(pageB, wasmBase64);

      await waitForSyncSession(pageB);
      await expect
        .poll(async () => await rootEntryPath(pageB), { timeout: 15000 })
        .not.toBeNull();
      const downloadedRootPath = await rootEntryPath(pageB);
      if (!downloadedRootPath) {
        throw new Error("Expected the downloaded workspace to expose a root entry path");
      }

      // Verify the downloaded workspace contains the unique workspace name
      // (embedded in the README title during workspace creation).
      await expect
        .poll(async () => await readEntryBody(pageB, downloadedRootPath), { timeout: 30000 })
        .toContain(remoteWorkspaceName);

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

      const renamedEntryPath = await renameEntry(
        pageA,
        createdEntryPath,
        renamedEntryFilename,
      );
      await openEntryForSync(pageA, renamedEntryPath);

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
      console.log("[sync-e2e] step: check body after move");
      await expect
        .poll(async () => await readEntryBody(pageB, movedEntryPath), { timeout: 30000 })
        .toContain(createdEntryMarker);

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
        }, {
          timeout: 30000,
        })
        .toBeGreaterThan(initialDestinationContentsCount);
      const expectedDestinationContents = await readFrontmatterProperty(
        pageA,
        destinationParentPath,
        "contents",
      );

      await expectFrontmatterProperty(pageB, movedEntryPath, "part_of", expectedMovedPartOf);
      await expectFrontmatterProperty(pageB, destinationParentPath, "contents", expectedDestinationContents);

      console.log("[sync-e2e] step: set frontmatter description");
      await setFrontmatterProperty(pageA, movedEntryPath, "description", descriptionMarker);
      console.log("[sync-e2e] step: check frontmatter synced");
      await expect
        .poll(async () => (await readFrontmatter(pageB, movedEntryPath))?.description ?? null, {
          timeout: 30000,
        })
        .toBe(descriptionMarker);

      // Edit while both clients are already subscribed to the moved entry.
      // This is the real-time propagation path that was previously untested.
      await appendMarkerToEntry(pageA, movedEntryPath, editedBodyMarker);

      // Verify pageA has the edited body before checking pageB.
      await expect
        .poll(async () => await readEntryBody(pageA, movedEntryPath), { timeout: 10000 })
        .toContain(editedBodyMarker);

      await expect
        .poll(async () => await readEntryBody(pageB, movedEntryPath), { timeout: 30000 })
        .toContain(editedBodyMarker);

      const expectedBodyAfterEdit = await readEntryBody(pageB, movedEntryPath);
      if (!expectedBodyAfterEdit) {
        throw new Error("Expected body content on page B before reload");
      }

      console.log("[sync-e2e] step: reload pageB");
      await pageB.reload();
      await waitForAppReady(pageB, 45000);
      console.log("[sync-e2e] step: wait for bridge after reload");
      await expect
        .poll(async () => {
          const hasBridge = await pageB.evaluate(() => !!(globalThis as any).__diaryx_e2e);
          return hasBridge;
        }, { timeout: 30000 })
        .toBe(true);
      console.log("[sync-e2e] step: open entry after reload");
      await openEntryForSync(pageB, movedEntryPath);
      console.log("[sync-e2e] step: check body persisted after reload");
      await expect
        .poll(async () => await readEntryBody(pageB, movedEntryPath), { timeout: 30000 })
        .toBe(expectedBodyAfterEdit);
    } finally {
      await Promise.allSettled([contextA.close(), contextB.close()]);
    }
  });
});
