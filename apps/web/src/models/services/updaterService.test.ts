import { describe, it, expect, vi, beforeEach } from "vitest";
import { toast } from "svelte-sonner";

import type { Backend } from "$lib/backend/interface";

import {
  checkForAppUpdatesInBackground,
  installAvailableAppUpdate,
} from "./updaterService";

describe("updaterService", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("skips the background check when the backend has no updater support", async () => {
    const backend = {} as Backend;

    await checkForAppUpdatesInBackground(backend);

    expect(toast.info).not.toHaveBeenCalled();
  });

  it("shows an install toast when an update is available", async () => {
    const backend = {
      checkForAppUpdate: vi.fn().mockResolvedValue({
        version: "1.4.0",
        body: "Fresh release notes",
      }),
    } as Partial<Backend> as Backend;

    await checkForAppUpdatesInBackground(backend);

    expect(backend.checkForAppUpdate).toHaveBeenCalledOnce();
    expect(toast.info).toHaveBeenCalledWith("Diaryx 1.4.0 is ready to install", {
      description: "Fresh release notes",
      duration: 15000,
      action: expect.objectContaining({
        label: "Install",
      }),
    });
  });

  it("dismisses the loading toast when install finds nothing to do", async () => {
    const backend = {
      installAppUpdate: vi.fn().mockResolvedValue(false),
    } as Partial<Backend> as Backend;

    const installed = await installAvailableAppUpdate(backend, "1.4.0");

    expect(installed).toBe(false);
    expect(toast.loading).toHaveBeenCalledWith("Installing Diaryx 1.4.0...");
    expect(toast.dismiss).toHaveBeenCalledWith("mock-toast-id");
  });

  it("updates the loading toast while the app restarts into the new version", async () => {
    const backend = {
      installAppUpdate: vi.fn().mockResolvedValue(true),
    } as Partial<Backend> as Backend;

    const installed = await installAvailableAppUpdate(backend, "1.4.0");

    expect(installed).toBe(true);
    expect(toast.loading).toHaveBeenNthCalledWith(1, "Installing Diaryx 1.4.0...");
    expect(toast.loading).toHaveBeenNthCalledWith(2, "Restarting Diaryx...", {
      id: "mock-toast-id",
    });
  });
});
