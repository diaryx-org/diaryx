import { describe, expect, it, vi } from "vitest";

import { executeProviderPluginCommand } from "./providerPluginCommands";

describe("providerPluginCommands", () => {
  it("adds provider_id to provider command params", async () => {
    const api = {
      executePluginCommand: vi.fn().mockResolvedValue({ ready: true }),
    } as any;

    const result = await executeProviderPluginCommand({
      api,
      pluginId: "diaryx.sync",
      command: "GetProviderStatus",
      params: {
        remote_id: "remote-1",
      },
    });

    expect(api.executePluginCommand).toHaveBeenCalledWith(
      "diaryx.sync",
      "GetProviderStatus",
      {
        provider_id: "diaryx.sync",
        remote_id: "remote-1",
      },
    );
    expect(result).toEqual({ ready: true });
  });

  it("supports provider commands without extra params", async () => {
    const api = {
      executePluginCommand: vi.fn().mockResolvedValue({ workspaces: [] }),
    } as any;

    await executeProviderPluginCommand({
      api,
      pluginId: "diaryx.sync",
      command: "ListRemoteWorkspaces",
    });

    expect(api.executePluginCommand).toHaveBeenCalledWith(
      "diaryx.sync",
      "ListRemoteWorkspaces",
      {
        provider_id: "diaryx.sync",
      },
    );
  });
});
