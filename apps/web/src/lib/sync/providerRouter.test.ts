import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  executeBuiltinIcloudProviderCommand: vi.fn(),
}));

vi.mock("./builtinIcloudProvider", () => ({
  executeBuiltinIcloudProviderCommand: mocks.executeBuiltinIcloudProviderCommand,
}));

import { executeProviderCommand } from "./providerRouter";

describe("providerRouter", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("routes plugin providers through executePluginCommand", async () => {
    const api = {
      executePluginCommand: vi.fn().mockResolvedValue({ ready: true }),
    } as any;

    const result = await executeProviderCommand({
      api,
      pluginId: "diaryx.sync",
      command: "GetProviderStatus",
      params: { remote_id: "remote-1" } as any,
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

  it("routes built-in providers through the host adapter", async () => {
    mocks.executeBuiltinIcloudProviderCommand.mockResolvedValue({ ready: false });
    const api = {
      executePluginCommand: vi.fn(),
    } as any;

    const result = await executeProviderCommand({
      api,
      pluginId: "builtin.icloud",
      command: "GetProviderStatus",
      params: { local_workspace_id: "local-1" } as any,
    });

    expect(mocks.executeBuiltinIcloudProviderCommand).toHaveBeenCalledWith({
      api,
      command: "GetProviderStatus",
      params: {
        provider_id: "builtin.icloud",
        local_workspace_id: "local-1",
      },
    });
    expect(api.executePluginCommand).not.toHaveBeenCalled();
    expect(result).toEqual({ ready: false });
  });
});
