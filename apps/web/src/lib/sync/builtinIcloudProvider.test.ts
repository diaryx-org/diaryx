import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  isAppleTauriRuntime: vi.fn(),
  getBackend: vi.fn(),
  invoke: vi.fn(),
}));

vi.mock("./builtinProviders", () => ({
  isAppleTauriRuntime: mocks.isAppleTauriRuntime,
  BUILTIN_ICLOUD_PROVIDER_ID: "builtin.icloud",
  ICLOUD_LOCAL_PREFIX: "builtin.icloud:",
  makeLocalIcloudRemoteId: (key: string) => `builtin.icloud:${key}`,
}));

vi.mock("$lib/backend", () => ({
  getBackend: mocks.getBackend,
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: mocks.invoke,
}));

import { executeBuiltinIcloudProviderCommand } from "./builtinIcloudProvider";

const fakeApi = {} as any;

describe("executeBuiltinIcloudProviderCommand", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe("GetProviderStatus", () => {
    it("returns not ready when not Apple Tauri runtime", async () => {
      mocks.isAppleTauriRuntime.mockReturnValue(false);

      const result = await executeBuiltinIcloudProviderCommand({
        api: fakeApi,
        command: "GetProviderStatus",
      });

      expect(result).toEqual({
        ready: false,
        message: "iCloud Drive is only available in Apple Tauri builds.",
      });
    });

    it("returns ready when iCloud is available", async () => {
      mocks.isAppleTauriRuntime.mockReturnValue(true);
      mocks.invoke.mockResolvedValue({
        isAvailable: true,
        hasWorkspace: true,
        workspacePath: "/icloud/ws",
        workspaceName: "My WS",
        active: true,
      });

      const result = await executeBuiltinIcloudProviderCommand({
        api: fakeApi,
        command: "GetProviderStatus",
      });

      expect(result).toEqual({ ready: true, message: null });
      expect(mocks.invoke).toHaveBeenCalledWith(
        "get_icloud_workspace_info",
        undefined,
      );
    });

    it("returns not ready when iCloud is not available on device", async () => {
      mocks.isAppleTauriRuntime.mockReturnValue(true);
      mocks.invoke.mockResolvedValue({
        isAvailable: false,
        hasWorkspace: false,
        active: false,
      });

      const result = await executeBuiltinIcloudProviderCommand({
        api: fakeApi,
        command: "GetProviderStatus",
      });

      expect(result).toEqual({
        ready: false,
        message:
          "iCloud is not available on this device. Sign in to iCloud in Settings.",
      });
    });

    it("returns not ready with error message when invoke throws", async () => {
      mocks.isAppleTauriRuntime.mockReturnValue(true);
      mocks.invoke.mockRejectedValue(new Error("Tauri IPC failed"));

      const result = await executeBuiltinIcloudProviderCommand({
        api: fakeApi,
        command: "GetProviderStatus",
      });

      expect(result).toEqual({
        ready: false,
        message: "Tauri IPC failed",
      });
    });

    it("stringifies non-Error throws", async () => {
      mocks.isAppleTauriRuntime.mockReturnValue(true);
      mocks.invoke.mockRejectedValue("raw string error");

      const result = await executeBuiltinIcloudProviderCommand({
        api: fakeApi,
        command: "GetProviderStatus",
      });

      expect(result).toEqual({
        ready: false,
        message: "raw string error",
      });
    });
  });

  describe("ListRemoteWorkspaces", () => {
    it("returns empty list when not Apple Tauri", async () => {
      mocks.isAppleTauriRuntime.mockReturnValue(false);

      const result = await executeBuiltinIcloudProviderCommand({
        api: fakeApi,
        command: "ListRemoteWorkspaces",
      });

      expect(result).toEqual({ workspaces: [] });
    });

    it("returns workspace when available", async () => {
      mocks.isAppleTauriRuntime.mockReturnValue(true);
      mocks.invoke.mockResolvedValue([
        {
          workspaceId: "/path/to/ws",
          workspaceName: "Test WS",
          workspacePath: "/path/to/ws",
          active: true,
        },
      ]);

      const result = await executeBuiltinIcloudProviderCommand<any>({
        api: fakeApi,
        command: "ListRemoteWorkspaces",
      });

      expect(result.workspaces).toEqual([
        { id: "/path/to/ws", name: "Test WS" },
      ]);
      expect(mocks.invoke).toHaveBeenCalledWith(
        "list_icloud_workspaces",
        undefined,
      );
    });

    it("returns empty list when no workspaces exist", async () => {
      mocks.isAppleTauriRuntime.mockReturnValue(true);
      mocks.invoke.mockResolvedValue([]);

      const result = await executeBuiltinIcloudProviderCommand<any>({
        api: fakeApi,
        command: "ListRemoteWorkspaces",
      });

      expect(result.workspaces).toEqual([]);
    });
  });

  describe("LinkWorkspace", () => {
    it("throws when not Apple Tauri", async () => {
      mocks.isAppleTauriRuntime.mockReturnValue(false);

      await expect(
        executeBuiltinIcloudProviderCommand({
          api: fakeApi,
          command: "LinkWorkspace",
        }),
      ).rejects.toThrow(
        "iCloud Drive is only available in Apple Tauri builds.",
      );
    });

    it("returns existing link when icloud is already active", async () => {
      mocks.isAppleTauriRuntime.mockReturnValue(true);
      mocks.getBackend.mockResolvedValue({
        getAppPaths: () => ({
          icloud_active: true,
          icloud_workspace: "/path/to/existing-ws",
        }),
      });

      const result = await executeBuiltinIcloudProviderCommand<any>({
        api: fakeApi,
        command: "LinkWorkspace",
      });

      expect(result).toEqual({
        remote_id: "builtin.icloud:existing-ws",
        created_remote: false,
        snapshot_uploaded: false,
      });
      expect(mocks.invoke).not.toHaveBeenCalledWith(
        "link_icloud_workspace",
        expect.anything(),
      );
    });

    it("uses BUILTIN_ICLOUD_PROVIDER_ID fallback when icloud_workspace is empty", async () => {
      mocks.isAppleTauriRuntime.mockReturnValue(true);
      mocks.getBackend.mockResolvedValue({
        getAppPaths: () => ({
          icloud_active: true,
          icloud_workspace: "",
        }),
      });

      const result = await executeBuiltinIcloudProviderCommand<any>({
        api: fakeApi,
        command: "LinkWorkspace",
      });

      expect(result.remote_id).toBe("builtin.icloud");
    });

    it("enables icloud when not yet active", async () => {
      mocks.isAppleTauriRuntime.mockReturnValue(true);
      mocks.getBackend.mockResolvedValue({
        getAppPaths: () => ({
          icloud_active: false,
        }),
      });
      mocks.invoke.mockResolvedValue({
        icloud_workspace: "/path/to/new-ws-path",
      });

      const result = await executeBuiltinIcloudProviderCommand<any>({
        api: fakeApi,
        command: "LinkWorkspace",
      });

      expect(mocks.invoke).toHaveBeenCalledWith("link_icloud_workspace", {
        workspaceId: null,
        workspaceName: null,
      });
      expect(result).toEqual({
        remote_id: "builtin.icloud:new-ws-path",
        created_remote: false,
        snapshot_uploaded: false,
      });
    });

    it("uses requested remote_id from params when provided", async () => {
      mocks.isAppleTauriRuntime.mockReturnValue(true);
      mocks.getBackend.mockResolvedValue({
        getAppPaths: () => ({ icloud_active: false }),
      });
      mocks.invoke.mockResolvedValue({
        icloud_workspace: "/some/path",
      });

      const result = await executeBuiltinIcloudProviderCommand<any>({
        api: fakeApi,
        command: "LinkWorkspace",
        params: { remote_id: "my-custom-id" as any },
      });

      expect(result.remote_id).toBe("my-custom-id");
    });

    it("uses BUILTIN_ICLOUD_PROVIDER_ID as last resort fallback", async () => {
      mocks.isAppleTauriRuntime.mockReturnValue(true);
      mocks.getBackend.mockResolvedValue({
        getAppPaths: () => ({ icloud_active: false }),
      });
      mocks.invoke.mockResolvedValue({});

      const result = await executeBuiltinIcloudProviderCommand<any>({
        api: fakeApi,
        command: "LinkWorkspace",
      });

      expect(result.remote_id).toBe("builtin.icloud");
    });
  });

  describe("UnlinkWorkspace", () => {
    it("invokes set_icloud_enabled with false", async () => {
      mocks.invoke.mockResolvedValue(undefined);

      const result = await executeBuiltinIcloudProviderCommand({
        api: fakeApi,
        command: "UnlinkWorkspace",
      });

      expect(mocks.invoke).toHaveBeenCalledWith("set_icloud_enabled", {
        enabled: false,
      });
      expect(result).toBeUndefined();
    });
  });

  describe("DownloadWorkspace", () => {
    it("invokes restore_icloud_workspace", async () => {
      mocks.invoke.mockResolvedValue(undefined);

      const result = await executeBuiltinIcloudProviderCommand<any>({
        api: fakeApi,
        command: "DownloadWorkspace",
      });

      expect(mocks.invoke).toHaveBeenCalledWith("restore_icloud_workspace", {
        workspaceId: null,
      });
      expect(result).toEqual({ files_imported: 0 });
    });

    it("passes remote_id from params as workspaceId", async () => {
      mocks.invoke.mockResolvedValue(undefined);

      await executeBuiltinIcloudProviderCommand({
        api: fakeApi,
        command: "DownloadWorkspace",
        params: { remote_id: "specific-ws" as any },
      });

      expect(mocks.invoke).toHaveBeenCalledWith("restore_icloud_workspace", {
        workspaceId: "specific-ws",
      });
    });
  });

  describe("unknown command", () => {
    it("throws for unsupported commands", async () => {
      await expect(
        executeBuiltinIcloudProviderCommand({
          api: fakeApi,
          command: "SomethingUnknown",
        }),
      ).rejects.toThrow(
        "Unsupported built-in iCloud provider command: SomethingUnknown",
      );
    });
  });
});
