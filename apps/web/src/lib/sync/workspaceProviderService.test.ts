import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  getBackend: vi.fn(),
  createApi: vi.fn(),
  resolveStorageType: vi.fn(),
  addLocalWorkspace: vi.fn(),
  setCurrentWorkspaceId: vi.fn(),
  setPluginMetadata: vi.fn(),
  createLocalWorkspace: vi.fn(),
  getLocalWorkspace: vi.fn(),
  getLocalWorkspaces: vi.fn(),
  getWorkspaceStorageType: vi.fn(),
  captureProviderPluginForTransfer: vi.fn(),
  installCapturedProviderPlugin: vi.fn(),
  inspectPluginWasm: vi.fn(),
  loadAllPlugins: vi.fn(),
  loadPluginWithCustomInit: vi.fn(),
}));

vi.mock("$lib/backend", () => ({
  getBackend: mocks.getBackend,
  createApi: mocks.createApi,
  isTauri: () => false,
  resetBackend: vi.fn(),
}));

vi.mock("$lib/backend/storageType", () => ({
  resolveStorageType: mocks.resolveStorageType,
}));

vi.mock("$lib/storage/localWorkspaceRegistry.svelte", () => ({
  addLocalWorkspace: mocks.addLocalWorkspace,
  setCurrentWorkspaceId: mocks.setCurrentWorkspaceId,
  setPluginMetadata: mocks.setPluginMetadata,
  createLocalWorkspace: mocks.createLocalWorkspace,
  getLocalWorkspace: mocks.getLocalWorkspace,
  getLocalWorkspaces: mocks.getLocalWorkspaces,
  getWorkspaceStorageType: mocks.getWorkspaceStorageType,
}));

vi.mock("$lib/sync/browserProviderBootstrap", () => ({
  captureProviderPluginForTransfer: mocks.captureProviderPluginForTransfer,
  installCapturedProviderPlugin: mocks.installCapturedProviderPlugin,
}));

vi.mock("$lib/plugins/browserPluginManager.svelte", () => ({
  inspectPluginWasm: mocks.inspectPluginWasm,
  loadAllPlugins: mocks.loadAllPlugins,
  loadPluginWithCustomInit: mocks.loadPluginWithCustomInit,
}));

vi.mock("$lib/auth", () => ({
  getServerUrl: vi.fn(() => null),
  getToken: vi.fn(() => null),
  listUserWorkspaceNamespaces: vi.fn(),
}));

import {
  attachExistingLocalWorkspaceToRemote,
  downloadWorkspace,
  linkWorkspace,
  listUnlinkedRemoteWorkspaces,
  uploadWorkspaceSnapshot,
} from "./workspaceProviderService";
import { listUserWorkspaceNamespaces } from "$lib/auth";

describe("workspaceProviderService", () => {
  let downloadedWorkspaceApi: any;

  beforeEach(() => {
    vi.clearAllMocks();

    mocks.getLocalWorkspace.mockReturnValue({
      id: "local-1",
      name: "Journal",
      path: "/tmp/journal",
    });
    mocks.resolveStorageType.mockResolvedValue("memory");
    mocks.createLocalWorkspace.mockReturnValue({ id: "local-1" });
    mocks.getLocalWorkspaces.mockReturnValue([]);
    mocks.getWorkspaceStorageType.mockReturnValue("memory");
    mocks.captureProviderPluginForTransfer.mockResolvedValue(new Uint8Array([1, 2, 3]));
    mocks.installCapturedProviderPlugin.mockResolvedValue(undefined);
    mocks.loadAllPlugins.mockResolvedValue(undefined);
    mocks.loadPluginWithCustomInit.mockResolvedValue(undefined);
    mocks.inspectPluginWasm.mockResolvedValue({
      pluginId: "diaryx.sync",
      requestedPermissions: undefined,
    });
    mocks.getBackend.mockResolvedValue({
      getWorkspacePath: () => "/tmp/remote-notes/index.md",
    });
    downloadedWorkspaceApi = {
      executePluginCommand: vi.fn().mockResolvedValue({ files_imported: 7 }),
      getPluginConfig: vi.fn().mockResolvedValue({}),
      setPluginConfig: vi.fn().mockResolvedValue(undefined),
      findRootIndex: vi.fn().mockResolvedValue("/tmp/remote-notes/index.md"),
      getFrontmatter: vi.fn().mockResolvedValue({}),
      setFrontmatterProperty: vi.fn().mockResolvedValue(null),
    };
    mocks.createApi.mockReturnValue(downloadedWorkspaceApi);
  });

  it("filters already-linked remote workspaces", async () => {
    const api = {
      executePluginCommand: vi.fn().mockResolvedValue({
        workspaces: [
          { id: "remote-1", name: "Journal" },
          { id: "remote-2", name: "Archive" },
        ],
      }),
    } as any;

    const result = await listUnlinkedRemoteWorkspaces(
      "test.provider",
      new Set(["remote-1"]),
      api,
    );

    expect(result).toEqual([{ id: "remote-2", name: "Archive" }]);
  });

  it("derives diaryx.sync remote workspaces from filtered account namespaces", async () => {
    vi.mocked(listUserWorkspaceNamespaces).mockResolvedValue([
      {
        id: "remote-1",
        owner_user_id: "u1",
        created_at: 1000,
        metadata: { type: "workspace", provider: "diaryx.sync", name: "Journal" },
      },
      {
        id: "remote-2",
        owner_user_id: "u1",
        created_at: 2000,
        metadata: { type: "workspace", name: "Fallback Provider" },
      },
      {
        id: "remote-3",
        owner_user_id: "u1",
        created_at: 3000,
        metadata: { type: "workspace", provider: "builtin.icloud", name: "Wrong Provider" },
      },
    ] as any);

    const result = await listUnlinkedRemoteWorkspaces(
      "diaryx.sync",
      new Set(["remote-1"]),
    );

    expect(result).toEqual([{ id: "remote-2", name: "Fallback Provider" }]);
  });

  it("links a workspace and persists sync metadata", async () => {
    const api = {
      executePluginCommand: vi.fn().mockResolvedValue({
        remote_id: "remote-1",
        created_remote: true,
        snapshot_uploaded: false,
      }),
    } as any;
    const onProgress = vi.fn();

    const result = await linkWorkspace(
      "diaryx.sync",
      { localId: "local-1", name: "Journal" },
      onProgress,
      api,
    );

    expect(api.executePluginCommand).toHaveBeenCalledWith(
      "diaryx.sync",
      "LinkWorkspace",
      {
        provider_id: "diaryx.sync",
        local_workspace_id: "local-1",
        name: "Journal",
        remote_id: null,
        workspace_root: "/tmp/journal",
      },
    );
    expect(mocks.setPluginMetadata).toHaveBeenCalledWith("local-1", "diaryx.sync", {
      remoteWorkspaceId: "remote-1",
      serverId: "remote-1",
      syncEnabled: true,
    });
    expect(onProgress).toHaveBeenNthCalledWith(1, {
      percent: 8,
      message: 'Starting sync for "Journal"...',
    });
    expect(onProgress).toHaveBeenNthCalledWith(2, {
      percent: 100,
      message: "Sync enabled.",
    });
    expect(result).toEqual({
      remoteId: "remote-1",
      createdRemote: true,
      snapshotUploaded: false,
    });
  });

  it("rejects invalid provider link responses", async () => {
    const api = {
      executePluginCommand: vi.fn().mockResolvedValue({
        remote_id: "",
      }),
    } as any;

    await expect(
      linkWorkspace("diaryx.sync", { localId: "local-1", name: "Journal" }, undefined, api),
    ).rejects.toThrow("Provider returned an invalid remote workspace ID");

    expect(mocks.setPluginMetadata).not.toHaveBeenCalled();
  });

  it("omits workspace_root when the local registry has no path for the workspace", async () => {
    mocks.getLocalWorkspace.mockReturnValue({
      id: "local-1",
      name: "Journal",
      path: null,
    });
    const api = {
      executePluginCommand: vi
        .fn()
        .mockResolvedValue({
          remote_id: "remote-1",
          created_remote: false,
          snapshot_uploaded: true,
        }),
    } as any;

    await linkWorkspace(
      "diaryx.sync",
      { localId: "local-1", name: "Journal" },
      undefined,
      api,
    );

    expect(api.executePluginCommand).toHaveBeenCalledWith(
      "diaryx.sync",
      "LinkWorkspace",
      {
        provider_id: "diaryx.sync",
        local_workspace_id: "local-1",
        name: "Journal",
        remote_id: null,
      },
    );
  });

  it("uploads snapshots with replace+attachments defaults", async () => {
    const api = {
      executePluginCommand: vi.fn().mockResolvedValue({
        files_uploaded: 12,
        snapshot_uploaded: true,
      }),
    } as any;

    const result = await uploadWorkspaceSnapshot(
      "diaryx.sync",
      { remoteId: "remote-1" },
      api,
    );

    expect(api.executePluginCommand).toHaveBeenCalledWith(
      "diaryx.sync",
      "UploadWorkspaceSnapshot",
      {
        provider_id: "diaryx.sync",
        remote_id: "remote-1",
        mode: "replace",
        include_attachments: true,
      },
    );
    expect(result).toEqual({
      filesUploaded: 12,
      snapshotUploaded: true,
    });
  });

  it("attaches an existing local workspace with metadata only", async () => {
    mocks.getLocalWorkspaces.mockReturnValue([
      {
        id: "local-9",
        name: "CLI Journal",
        path: "/tmp/cli-journal",
      },
    ]);

    const result = await attachExistingLocalWorkspaceToRemote(
      "diaryx.sync",
      {
        remoteId: "remote-9",
        remoteName: "Journal",
        localPath: "/tmp/cli-journal",
        policy: "link_only",
      },
    );

    expect(mocks.createLocalWorkspace).not.toHaveBeenCalled();
    expect(mocks.addLocalWorkspace).toHaveBeenCalledWith({
      id: "local-9",
      name: "CLI Journal",
      path: "/tmp/cli-journal",
    });
    expect(mocks.setPluginMetadata).toHaveBeenCalledWith("local-9", "diaryx.sync", {
      remoteWorkspaceId: "remote-9",
      serverId: "remote-9",
      syncEnabled: true,
    });
    expect(result).toEqual({
      localId: "local-9",
      localName: "CLI Journal",
      remoteId: "remote-9",
      snapshotUploaded: false,
    });
  });

  it("creates a local registry entry and uploads when linkWorkspace skips snapshot upload", async () => {
    const api = {
      executePluginCommand: vi
        .fn()
        .mockResolvedValueOnce({
          remote_id: "remote-11",
          created_remote: false,
          snapshot_uploaded: false,
        })
        .mockResolvedValueOnce({
          files_uploaded: 4,
          snapshot_uploaded: true,
        }),
    } as any;
    mocks.getLocalWorkspaces.mockReturnValue([]);
    mocks.createLocalWorkspace.mockReturnValue({
      id: "local-11",
      name: "Journal",
      path: "/tmp/linked-journal",
    });
    mocks.getLocalWorkspace.mockReturnValue({
      id: "local-11",
      name: "Journal",
      path: "/tmp/linked-journal",
    });

    const result = await attachExistingLocalWorkspaceToRemote(
      "diaryx.sync",
      {
        remoteId: "remote-11",
        remoteName: "Journal",
        localPath: "/tmp/linked-journal",
        policy: "upload_local",
      },
      undefined,
      api,
    );

    expect(mocks.createLocalWorkspace).toHaveBeenCalledWith(
      "Journal",
      undefined,
      "/tmp/linked-journal",
    );
    expect(api.executePluginCommand).toHaveBeenNthCalledWith(
      1,
      "diaryx.sync",
      "LinkWorkspace",
      {
        provider_id: "diaryx.sync",
        local_workspace_id: "local-11",
        name: "Journal",
        remote_id: "remote-11",
        workspace_root: "/tmp/linked-journal",
      },
    );
    expect(api.executePluginCommand).toHaveBeenNthCalledWith(
      2,
      "diaryx.sync",
      "UploadWorkspaceSnapshot",
      {
        provider_id: "diaryx.sync",
        remote_id: "remote-11",
        mode: "replace",
        include_attachments: true,
      },
    );
    expect(result).toEqual({
      localId: "local-11",
      localName: "Journal",
      remoteId: "remote-11",
      snapshotUploaded: true,
    });
  });

  it("downloads a remote workspace, registers it locally, and links when requested", async () => {
    const onProgress = vi.fn();

    const result = await downloadWorkspace(
      "diaryx.sync",
      { remoteId: "remote-1", name: "Remote Notes", link: true },
      onProgress,
    );

    expect(mocks.captureProviderPluginForTransfer).toHaveBeenCalledWith("diaryx.sync");
    expect(mocks.addLocalWorkspace).toHaveBeenNthCalledWith(1, {
      id: "local-1",
      name: "Remote Notes",
    });
    expect(mocks.setCurrentWorkspaceId).toHaveBeenCalledWith("local-1");
    expect(mocks.getBackend).toHaveBeenCalledWith("local-1", "Remote Notes", "memory", undefined, undefined);
    expect(mocks.addLocalWorkspace).toHaveBeenNthCalledWith(2, {
      id: "local-1",
      name: "Remote Notes",
      path: "/tmp/remote-notes",
    });
    expect(mocks.installCapturedProviderPlugin).toHaveBeenCalledWith(
      "diaryx.sync",
      new Uint8Array([1, 2, 3]),
    );
    expect(mocks.loadPluginWithCustomInit).toHaveBeenCalledWith(
      new Uint8Array([1, 2, 3]).buffer,
      {
        workspace_root: "/tmp/remote-notes",
        workspace_id: "local-1",
        write_to_disk: true,
        server_url: null,
        auth_token: null,
      },
    );
    expect(mocks.loadAllPlugins).not.toHaveBeenCalled();
    expect(downloadedWorkspaceApi.executePluginCommand).toHaveBeenCalledWith(
      "diaryx.sync",
      "DownloadWorkspace",
      {
        provider_id: "diaryx.sync",
        workspace_root: "/tmp/remote-notes",
        remote_id: "remote-1",
        link: true,
      },
    );
    expect(mocks.setPluginMetadata).toHaveBeenCalledWith("local-1", "diaryx.sync", {
      remoteWorkspaceId: "remote-1",
      serverId: "remote-1",
      syncEnabled: true,
    });
    expect(onProgress).toHaveBeenNthCalledWith(1, {
      percent: 10,
      message: "Creating local workspace...",
    });
    expect(onProgress).toHaveBeenNthCalledWith(2, {
      percent: 40,
      message: "Downloading workspace...",
    });
    expect(onProgress).toHaveBeenNthCalledWith(3, {
      percent: 100,
      message: "Done.",
    });
    expect(result).toEqual({
      localId: "local-1",
      filesImported: 7,
    });
  });

  it("downloads without enabling sync metadata when link is false", async () => {
    downloadedWorkspaceApi.executePluginCommand.mockResolvedValue({ files_imported: 2 });

    await downloadWorkspace(
      "diaryx.sync",
      { remoteId: "remote-1", name: "Remote Notes", link: false },
    );

    expect(mocks.setPluginMetadata).not.toHaveBeenCalled();
  });

  it("persists transferred plugin default permissions into the downloaded workspace", async () => {
    const workspaceApi = {
      executePluginCommand: vi.fn().mockResolvedValue({ files_imported: 2 }),
      getPluginConfig: vi.fn().mockResolvedValue({}),
      setPluginConfig: vi.fn().mockResolvedValue(undefined),
      findRootIndex: vi.fn().mockResolvedValue("/tmp/remote-notes/index.md"),
      getFrontmatter: vi.fn().mockResolvedValue({}),
      setFrontmatterProperty: vi.fn().mockResolvedValue(null),
    };
    mocks.createApi.mockReturnValue(workspaceApi);
    mocks.inspectPluginWasm.mockResolvedValue({
      pluginId: "diaryx.sync",
      requestedPermissions: {
        defaults: {
          http_requests: { include: ["all"], exclude: [] },
          read_files: { include: ["all"], exclude: [] },
        },
      },
    });

    await downloadWorkspace(
      "diaryx.sync",
      { remoteId: "remote-1", name: "Remote Notes", link: false },
    );

    expect(mocks.inspectPluginWasm).toHaveBeenCalled();
    expect(workspaceApi.findRootIndex).toHaveBeenCalledWith("/tmp/remote-notes");
    expect(workspaceApi.setFrontmatterProperty).toHaveBeenCalledWith(
      "/tmp/remote-notes/index.md",
      "plugins",
      {
        "diaryx.sync": {
          permissions: {
            http_requests: { include: ["all"], exclude: [] },
            read_files: { include: ["all"], exclude: [] },
          },
        },
      },
      "/tmp/remote-notes/index.md",
    );
  });

  it("uses pre-fetched pluginWasm bytes instead of capturing from current workspace", async () => {
    const preFetchedWasm = new Uint8Array([10, 20, 30]);

    await downloadWorkspace(
      "diaryx.sync",
      { remoteId: "remote-1", name: "Remote Notes", link: true },
      undefined,
      undefined,
      preFetchedWasm,
    );

    // Should NOT try to capture from current workspace
    expect(mocks.captureProviderPluginForTransfer).not.toHaveBeenCalled();
    // Should install the pre-fetched bytes
    expect(mocks.installCapturedProviderPlugin).toHaveBeenCalledWith(
      "diaryx.sync",
      preFetchedWasm,
    );
    expect(mocks.loadPluginWithCustomInit).toHaveBeenCalledWith(
      preFetchedWasm.buffer,
      {
        workspace_root: "/tmp/remote-notes",
        workspace_id: "local-1",
        write_to_disk: true,
        server_url: null,
        auth_token: null,
      },
    );
    expect(mocks.loadAllPlugins).not.toHaveBeenCalled();
    expect(downloadedWorkspaceApi.executePluginCommand).toHaveBeenCalledWith(
      "diaryx.sync",
      "DownloadWorkspace",
      expect.objectContaining({
        remote_id: "remote-1",
      }),
    );
  });
});
