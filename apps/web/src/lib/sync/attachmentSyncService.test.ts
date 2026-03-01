import { beforeEach, describe, expect, it, vi } from "vitest";
import type { BinaryRef } from "$lib/backend/generated";
import { AuthError } from "$lib/auth/authService";

describe("attachmentSyncService", () => {
  beforeEach(() => {
    vi.resetModules();
    localStorage.clear();
  });

  it("computes stable sha256 hashes", async () => {
    const service = await import("./attachmentSyncService");
    const hash = await service.sha256Hex(new TextEncoder().encode("hello"));
    expect(hash).toBe("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
  });

  it("deduplicates upload jobs by attachment key", async () => {
    const service = await import("./attachmentSyncService");
    service.enqueueAttachmentUpload({
      workspaceId: "ws-1",
      entryPath: "workspace/day.md",
      attachmentPath: "_attachments/a.png",
      hash: "a".repeat(64),
      mimeType: "image/png",
      sizeBytes: 123,
    });
    service.enqueueAttachmentUpload({
      workspaceId: "ws-1",
      entryPath: "workspace/day.md",
      attachmentPath: "_attachments/a.png",
      hash: "a".repeat(64),
      mimeType: "image/png",
      sizeBytes: 123,
    });

    const queue = service.getAttachmentSyncQueueSnapshot();
    expect(queue).toHaveLength(1);
    expect(queue[0].kind).toBe("upload");
  });

  it("indexes BinaryRefs and enqueues missing download requests", async () => {
    const service = await import("./attachmentSyncService");
    const refs: BinaryRef[] = [
      {
        path: "_attachments/a.png",
        source: "local",
        hash: "b".repeat(64),
        mime_type: "image/png",
        size: 321n,
        uploaded_at: 1n,
        deleted: false,
      },
    ];
    service.indexAttachmentRefs("workspace/day.md", refs, "ws-1");
    const queued = service.requestMissingBlobDownload(
      "workspace/day.md",
      "_attachments/a.png",
    );
    expect(queued).toBe(true);

    const queue = service.getAttachmentSyncQueueSnapshot();
    expect(queue).toHaveLength(1);
    expect(queue[0].kind).toBe("download");
    expect(queue[0].hash).toBe("b".repeat(64));
  });

  it("treats quota errors as terminal", async () => {
    const service = await import("./attachmentSyncService");
    expect(
      service.isTerminalAttachmentSyncError(new AuthError("quota", 413)),
    ).toBe(true);
    expect(
      service.isTerminalAttachmentSyncError(new AuthError("other", 500)),
    ).toBe(false);
  });
});
