import { beforeEach, describe, expect, it, vi } from "vitest";

const {
  getCurrentWorkspaceIdMock,
  getServerWorkspaceIdMock,
  isWorkspaceSyncEnabledMock,
  enqueueAttachmentUploadMock,
  isAttachmentSyncEnabledMock,
  sha256HexMock,
  indexAttachmentRefsMock,
} = vi.hoisted(() => ({
  getCurrentWorkspaceIdMock: vi.fn(),
  getServerWorkspaceIdMock: vi.fn(),
  isWorkspaceSyncEnabledMock: vi.fn(() => true),
  enqueueAttachmentUploadMock: vi.fn(),
  isAttachmentSyncEnabledMock: vi.fn(() => false),
  sha256HexMock: vi.fn(),
  indexAttachmentRefsMock: vi.fn(),
}));

vi.mock("../models/stores", () => ({
  entryStore: {
    setCurrentEntry: vi.fn(),
  },
}));

vi.mock("../models/services/attachmentService", () => ({
  trackBlobUrl: vi.fn(),
  computeRelativeAttachmentPath: vi.fn(),
  formatMarkdownDestination: vi.fn((path: string) => path),
  getMimeType: vi.fn(() => "application/octet-stream"),
  getAttachmentMediaKind: vi.fn(() => "file"),
  isPreviewableAttachmentKind: vi.fn(() => false),
}));

vi.mock("$lib/sync/attachmentSyncService", () => ({
  enqueueAttachmentUpload: enqueueAttachmentUploadMock,
  isAttachmentSyncEnabled: isAttachmentSyncEnabledMock,
  onQueueItemStateChange: vi.fn(),
  indexAttachmentRefs: indexAttachmentRefsMock,
  sha256Hex: sha256HexMock,
}));

vi.mock("$lib/storage/localWorkspaceRegistry.svelte", () => ({
  getCurrentWorkspaceId: getCurrentWorkspaceIdMock,
  getServerWorkspaceId: getServerWorkspaceIdMock,
  isWorkspaceSyncEnabled: isWorkspaceSyncEnabledMock,
}));

import { enqueueIncrementalAttachmentUpload } from "./attachmentController";

describe("enqueueIncrementalAttachmentUpload", () => {
  const makeMockFile = (bytes: number[]): File =>
    ({
      name: "image.png",
      type: "image/png",
      size: bytes.length,
      arrayBuffer: async () => Uint8Array.from(bytes).buffer,
    }) as File;

  beforeEach(() => {
    vi.clearAllMocks();
    sha256HexMock.mockResolvedValue("a".repeat(64));
    getCurrentWorkspaceIdMock.mockReturnValue("local-workspace-1");
    getServerWorkspaceIdMock.mockReturnValue("server-workspace-1");
    isWorkspaceSyncEnabledMock.mockReturnValue(true);
  });

  it("indexes attachment metadata and enqueues upload for synced workspaces", async () => {
    const file = makeMockFile([1, 2, 3]);

    await enqueueIncrementalAttachmentUpload(
      "my-journal.md",
      "my-journal/_attachments/image.png",
      file
    );

    expect(indexAttachmentRefsMock).toHaveBeenCalledTimes(1);

    expect(enqueueAttachmentUploadMock).toHaveBeenCalledTimes(1);
    expect(enqueueAttachmentUploadMock).toHaveBeenCalledWith({
      workspaceId: "server-workspace-1",
      entryPath: "my-journal.md",
      attachmentPath: "my-journal/_attachments/image.png",
      hash: "a".repeat(64),
      mimeType: "image/png",
      sizeBytes: 3,
    });
  });

  it("skips upload when the current workspace is not linked to sync", async () => {
    getCurrentWorkspaceIdMock.mockReturnValue("local-workspace-1");
    getServerWorkspaceIdMock.mockReturnValue(null);
    isWorkspaceSyncEnabledMock.mockReturnValue(true);

    const file = makeMockFile([7, 8, 9]);
    const arrayBufferSpy = vi.spyOn(file, "arrayBuffer");

    await enqueueIncrementalAttachmentUpload(
      "my-journal.md",
      "my-journal/_attachments/image.png",
      file
    );

    expect(indexAttachmentRefsMock).not.toHaveBeenCalled();
    expect(enqueueAttachmentUploadMock).not.toHaveBeenCalled();
    expect(sha256HexMock).not.toHaveBeenCalled();
    expect(arrayBufferSpy).not.toHaveBeenCalled();
  });
});
