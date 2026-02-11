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
});
