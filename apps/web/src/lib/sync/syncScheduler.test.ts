import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const registryMocks = vi.hoisted(() => ({
  getCurrentWorkspaceId: vi.fn(),
  getWorkspaceProviderLinks: vi.fn(),
}));

const pluginObserverHolder = vi.hoisted(() => ({
  observer: null as ((event: { event_type: string }) => void) | null,
}));

const pluginMocks = vi.hoisted(() => ({
  dispatchCommand: vi.fn(),
  onPluginEventDispatched: vi.fn((observer: (event: { event_type: string }) => void) => {
    pluginObserverHolder.observer = observer;
    return () => {
      pluginObserverHolder.observer = null;
    };
  }),
}));

vi.mock("$lib/storage/localWorkspaceRegistry.svelte", () => registryMocks);

vi.mock("$lib/plugins/browserPluginManager.svelte", () => pluginMocks);

import { runManualSyncNow, startSyncScheduler, stopSyncScheduler } from "./syncScheduler";

function setVisibilityState(state: DocumentVisibilityState): void {
  Object.defineProperty(document, "visibilityState", {
    configurable: true,
    value: state,
  });
}

async function flushMicrotasks(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
}

describe("syncScheduler", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    pluginObserverHolder.observer = null;
    registryMocks.getCurrentWorkspaceId.mockReturnValue("local-1");
    registryMocks.getWorkspaceProviderLinks.mockReturnValue([
      {
        pluginId: "diaryx.sync",
        remoteWorkspaceId: "remote-1",
        syncEnabled: true,
      },
    ]);
    pluginMocks.dispatchCommand.mockResolvedValue({ success: true });
    setVisibilityState("visible");
  });

  afterEach(() => {
    stopSyncScheduler();
    vi.useRealTimers();
  });

  it("runs a full sync immediately when the scheduler starts", async () => {
    startSyncScheduler();
    await flushMicrotasks();

    expect(pluginMocks.dispatchCommand).toHaveBeenCalledWith("diaryx.sync", "Sync", {
      provider_id: "diaryx.sync",
    });
  });

  it("debounces file mutation events into a full sync", async () => {
    startSyncScheduler();
    await flushMicrotasks();
    vi.clearAllMocks();

    pluginObserverHolder.observer?.({ event_type: "file_saved" });
    pluginObserverHolder.observer?.({ event_type: "file_created" });

    await vi.advanceTimersByTimeAsync(2_999);
    expect(pluginMocks.dispatchCommand).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(1);
    await flushMicrotasks();

    expect(pluginMocks.dispatchCommand).toHaveBeenCalledTimes(1);
    expect(pluginMocks.dispatchCommand).toHaveBeenCalledWith("diaryx.sync", "Sync", {
      provider_id: "diaryx.sync",
    });
  });

  it("re-runs sync when the tab becomes visible again", async () => {
    startSyncScheduler();
    await flushMicrotasks();
    vi.clearAllMocks();

    setVisibilityState("hidden");
    document.dispatchEvent(new Event("visibilitychange"));
    await flushMicrotasks();
    expect(pluginMocks.dispatchCommand).not.toHaveBeenCalled();

    setVisibilityState("visible");
    document.dispatchEvent(new Event("visibilitychange"));
    await flushMicrotasks();

    expect(pluginMocks.dispatchCommand).toHaveBeenCalledTimes(1);
    expect(pluginMocks.dispatchCommand).toHaveBeenCalledWith("diaryx.sync", "Sync", {
      provider_id: "diaryx.sync",
    });
  });

  it("manual sync runs immediately and cancels any pending debounce", async () => {
    startSyncScheduler();
    await flushMicrotasks();
    vi.clearAllMocks();

    pluginObserverHolder.observer?.({ event_type: "file_saved" });

    await runManualSyncNow();
    await flushMicrotasks();

    expect(pluginMocks.dispatchCommand).toHaveBeenCalledTimes(1);

    await vi.advanceTimersByTimeAsync(3_000);
    await flushMicrotasks();

    expect(pluginMocks.dispatchCommand).toHaveBeenCalledTimes(1);
  });
});
