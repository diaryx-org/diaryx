import { beforeEach, describe, expect, it, vi } from "vitest";
import { createAuthService } from "./authService";

describe("authService quota errors", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("parses quota payload for snapshot upload", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: false,
        status: 413,
        headers: {
          get: () => "application/json",
        },
        json: async () => ({
          error: "storage_limit_exceeded",
          message: "Attachment storage limit exceeded",
          used_bytes: 1024,
          limit_bytes: 512,
          requested_bytes: 100,
        }),
      }),
    );

    const service = createAuthService("http://localhost:3030");
    await expect(
      service.uploadWorkspaceSnapshot("token", "workspace", new Blob(["x"])),
    ).rejects.toMatchObject({
      statusCode: 413,
      message: expect.stringContaining("Attachment storage limit exceeded"),
    });
  });

  it("parses quota payload for init attachment upload", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: false,
        status: 413,
        headers: {
          get: () => "application/json",
        },
        json: async () => ({
          error: "storage_limit_exceeded",
          message: "Attachment storage limit exceeded",
          used_bytes: 10,
          limit_bytes: 9,
          requested_bytes: 2,
        }),
      }),
    );

    const service = createAuthService("http://localhost:3030");
    await expect(
      service.initAttachmentUpload("token", "workspace", {
        attachment_path: "_attachments/a.png",
        hash: "a".repeat(64),
        size_bytes: 2,
        mime_type: "image/png",
      }),
    ).rejects.toMatchObject({
      statusCode: 413,
      message: expect.stringContaining("Attachment storage limit exceeded"),
    });
  });

  it("parses quota payload for complete attachment upload", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: false,
        status: 413,
        headers: {
          get: () => "application/json",
        },
        json: async () => ({
          error: "storage_limit_exceeded",
          message: "Attachment storage limit exceeded",
          used_bytes: 10,
          limit_bytes: 9,
          requested_bytes: 2,
        }),
      }),
    );

    const service = createAuthService("http://localhost:3030");
    await expect(
      service.completeAttachmentUpload("token", "workspace", "upload-id", {
        attachment_path: "_attachments/a.png",
        hash: "a".repeat(64),
        size_bytes: 2,
        mime_type: "image/png",
      }),
    ).rejects.toMatchObject({
      statusCode: 413,
      message: expect.stringContaining("Attachment storage limit exceeded"),
    });
  });

  it("fetches user storage usage without cache", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({
        used_bytes: 100,
        blob_count: 2,
        limit_bytes: 1024,
        warning_threshold: 0.8,
        over_limit: false,
        scope: "attachments",
      }),
    });
    vi.stubGlobal("fetch", fetchMock);

    const service = createAuthService("http://localhost:3030");
    await service.getUserStorageUsage("token");

    expect(fetchMock).toHaveBeenCalledWith(
      "http://localhost:3030/api/user/storage",
      expect.objectContaining({
        cache: "no-store",
        headers: expect.objectContaining({
          Authorization: "Bearer token",
          "Cache-Control": "no-cache",
          Pragma: "no-cache",
        }),
      }),
    );
  });
});
