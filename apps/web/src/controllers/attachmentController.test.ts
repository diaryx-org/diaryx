import { beforeEach, describe, expect, it, vi } from "vitest";

const {
  getFileMetadataMock,
  setFileMetadataMock,
  getWorkspaceIdMock,
  getDefaultWorkspaceMock,
  enqueueAttachmentUploadMock,
  sha256HexMock,
} = vi.hoisted(() => ({
  getFileMetadataMock: vi.fn(),
  setFileMetadataMock: vi.fn(),
  getWorkspaceIdMock: vi.fn(),
  getDefaultWorkspaceMock: vi.fn(),
  enqueueAttachmentUploadMock: vi.fn(),
  sha256HexMock: vi.fn(),
}));

vi.mock("../models/stores", () => ({
  entryStore: {
    setCurrentEntry: vi.fn(),
  },
}));

vi.mock("../models/services/attachmentService", () => ({
  trackBlobUrl: vi.fn(),
  computeRelativeAttachmentPath: vi.fn(),
  getMimeType: vi.fn(() => "application/octet-stream"),
  bytesToBase64: vi.fn(),
}));

vi.mock("../models/services/attachmentSyncService", () => ({
  enqueueAttachmentUpload: enqueueAttachmentUploadMock,
  indexAttachmentRefs: vi.fn(),
  sha256Hex: sha256HexMock,
}));

vi.mock("../lib/auth/authStore.svelte", () => ({
  getDefaultWorkspace: getDefaultWorkspaceMock,
}));

vi.mock("../lib/crdt", () => ({
  getFileMetadata: getFileMetadataMock,
  getWorkspaceId: getWorkspaceIdMock,
  setFileMetadata: setFileMetadataMock,
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
    getFileMetadataMock.mockResolvedValue({
      filename: "my-journal.md",
      title: "My Journal",
      part_of: null,
      contents: null,
      attachments: [
        {
          path: "my-journal/_attachments/image.png",
          source: "local",
          hash: "",
          mime_type: "",
          size: 0n,
          uploaded_at: null,
          deleted: false,
        },
      ],
      deleted: false,
      audience: null,
      description: null,
      extra: {},
      modified_at: 1n,
    });
    setFileMetadataMock.mockResolvedValue(undefined);
  });

  it("updates BinaryRef metadata using canonical attachment path and enqueues with upload path", async () => {
    getWorkspaceIdMock.mockReturnValue("ws-active");
    getDefaultWorkspaceMock.mockReturnValue({ id: "ws-default", name: "default" });

    const file = makeMockFile([1, 2, 3]);

    await enqueueIncrementalAttachmentUpload(
      "my-journal.md",
      "my-journal/_attachments/image.png",
      file
    );

    expect(setFileMetadataMock).toHaveBeenCalledTimes(1);
    const updatedMetadata = setFileMetadataMock.mock.calls[0][1];
    expect(updatedMetadata.attachments[0].path).toBe("my-journal/_attachments/image.png");
    expect(updatedMetadata.attachments[0].hash).toBe("a".repeat(64));
    expect(updatedMetadata.attachments[0].mime_type).toBe("image/png");

    expect(enqueueAttachmentUploadMock).toHaveBeenCalledTimes(1);
    expect(enqueueAttachmentUploadMock).toHaveBeenCalledWith({
      workspaceId: "ws-active",
      entryPath: "my-journal.md",
      attachmentPath: "my-journal/_attachments/image.png",
      hash: "a".repeat(64),
      mimeType: "image/png",
      sizeBytes: 3,
    });
  });

  it("still updates CRDT hash metadata when workspace id is unavailable", async () => {
    getWorkspaceIdMock.mockReturnValue(null);
    getDefaultWorkspaceMock.mockReturnValue(null);

    const file = makeMockFile([7, 8, 9]);

    await enqueueIncrementalAttachmentUpload(
      "my-journal.md",
      "my-journal/_attachments/image.png",
      file
    );

    expect(setFileMetadataMock).toHaveBeenCalledTimes(1);
    expect(enqueueAttachmentUploadMock).not.toHaveBeenCalled();
  });
});
