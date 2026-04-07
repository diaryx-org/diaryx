import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const registryMocks = vi.hoisted(() => ({
  getCurrentWorkspaceId: vi.fn(),
  getWorkspaceProviderLinks: vi.fn(),
}));

const pluginMocks = vi.hoisted(() => ({
  onPluginEventDispatched: vi.fn(() => vi.fn()),
  dispatchCommand: vi.fn(),
}));

const backendMocks = vi.hoisted(() => {
  let resolveBackend!: (backend: {
    onFileSystemEvent: ReturnType<typeof vi.fn>;
    offFileSystemEvent: ReturnType<typeof vi.fn>;
  }) => void;
  let backendPromise = new Promise<{
    onFileSystemEvent: ReturnType<typeof vi.fn>;
    offFileSystemEvent: ReturnType<typeof vi.fn>;
  }>((resolve) => {
    resolveBackend = resolve;
  });
  const executePluginCommand = vi.fn().mockResolvedValue(undefined);
  return {
    createApi: vi.fn(() => ({ executePluginCommand })),
    executePluginCommand,
    getBackend: vi.fn(() => backendPromise),
    resetBackendPromise() {
      backendPromise = new Promise((resolve) => {
        resolveBackend = resolve;
      });
    },
    resolveBackend(backend: {
      onFileSystemEvent: ReturnType<typeof vi.fn>;
      offFileSystemEvent: ReturnType<typeof vi.fn>;
    }) {
      resolveBackend(backend);
    },
  };
});

vi.mock("$lib/storage/localWorkspaceRegistry.svelte", () => registryMocks);

vi.mock("$lib/plugins/browserPluginManager.svelte", () => pluginMocks);

vi.mock("$lib/backend/interface", () => ({
  isBrowser: () => false,
  isTauri: () => true,
}));

vi.mock("$lib/backend", () => ({
  createApi: backendMocks.createApi,
  getBackend: backendMocks.getBackend,
}));

vi.mock("$lib/backend/index", () => ({
  createApi: backendMocks.createApi,
  getBackend: backendMocks.getBackend,
}));

vi.mock("/src/lib/backend/index.ts", () => ({
  createApi: backendMocks.createApi,
  getBackend: backendMocks.getBackend,
}));

let scheduler: typeof import("./syncScheduler.svelte");

function createBackend(): {
  onFileSystemEvent: ReturnType<typeof vi.fn>;
  offFileSystemEvent: ReturnType<typeof vi.fn>;
} {
  return {
    onFileSystemEvent: vi.fn(() => 7),
    offFileSystemEvent: vi.fn(),
  };
}

async function flushMicrotasks(): Promise<void> {
  for (let i = 0; i < 10; i++) {
    await Promise.resolve();
  }
}

describe("syncScheduler Tauri lifecycle", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    (globalThis as { isTauri?: boolean }).isTauri = true;
    backendMocks.resetBackendPromise();
    registryMocks.getCurrentWorkspaceId.mockReturnValue("local-1");
    registryMocks.getWorkspaceProviderLinks.mockReturnValue([]);
    scheduler = await import("./syncScheduler.svelte");
  });

  afterEach(() => {
    scheduler?.stopSyncScheduler();
    delete (globalThis as { isTauri?: boolean }).isTauri;
  });

  it("does not install a late Tauri filesystem listener after stop", async () => {
    const backend = createBackend();

    scheduler.startSyncScheduler();
    scheduler.stopSyncScheduler();
    backendMocks.resolveBackend(backend);
    await flushMicrotasks();

    expect(backend.onFileSystemEvent).not.toHaveBeenCalled();
  });

  it("unsubscribes the captured Tauri filesystem listener on stop", async () => {
    const backend = createBackend();

    scheduler.startSyncScheduler();
    backendMocks.resolveBackend(backend);

    await vi.waitFor(() => {
      expect(backend.onFileSystemEvent).toHaveBeenCalledTimes(1);
    });

    scheduler.stopSyncScheduler();

    expect(backend.offFileSystemEvent).toHaveBeenCalledWith(7);
  });
});
