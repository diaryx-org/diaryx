import { beforeEach, describe, expect, it, vi } from "vitest";
import { AuthError, createAuthService, type AuthService } from "./authService";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Build a minimal mock Response that proxyFetch (via fetch) would return. */
function mockResponse(
  body: unknown,
  opts: { status?: number; ok?: boolean; contentType?: string } = {},
) {
  const status = opts.status ?? 200;
  const ok = opts.ok ?? (status >= 200 && status < 300);
  return {
    ok,
    status,
    headers: {
      get: (name: string) => {
        if (name.toLowerCase() === "content-type")
          return opts.contentType ?? "application/json";
        return null;
      },
    },
    json: async () => body,
    blob: async () => new Blob([JSON.stringify(body)]),
    arrayBuffer: async () => new TextEncoder().encode(JSON.stringify(body)).buffer,
    text: async () => JSON.stringify(body),
  };
}

function stubFetch(response: ReturnType<typeof mockResponse>) {
  const mock = vi.fn().mockResolvedValue(response);
  vi.stubGlobal("fetch", mock);
  return mock;
}

const SERVER = "http://localhost:3030";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("AuthService", () => {
  let service: AuthService;

  beforeEach(() => {
    vi.restoreAllMocks();
    service = createAuthService(SERVER);
  });

  // -----------------------------------------------------------------------
  // requestMagicLink
  // -----------------------------------------------------------------------
  describe("requestMagicLink", () => {
    it("sends POST with email and returns success response", async () => {
      const payload = { success: true, message: "Magic link sent" };
      const fetchMock = stubFetch(mockResponse(payload));

      const result = await service.requestMagicLink("user@example.com");

      expect(result).toEqual(payload);
      expect(fetchMock).toHaveBeenCalledWith(
        `${SERVER}/auth/magic-link`,
        expect.objectContaining({
          method: "POST",
          credentials: "include",
        }),
      );
      // Verify JSON body
      const callInit = fetchMock.mock.calls[0][1];
      expect(JSON.parse(callInit.body)).toEqual({ email: "user@example.com" });
    });

    it("throws AuthError with server error message on failure", async () => {
      stubFetch(mockResponse({ error: "Rate limited" }, { status: 429, ok: false }));

      await expect(service.requestMagicLink("user@example.com")).rejects.toThrow(AuthError);
      await expect(service.requestMagicLink("user@example.com")).rejects.toMatchObject({
        statusCode: 429,
        message: "Rate limited",
      });
    });

    it("uses fallback message when server error field is absent", async () => {
      stubFetch(mockResponse({}, { status: 500, ok: false }));

      await expect(service.requestMagicLink("user@example.com")).rejects.toMatchObject({
        message: "Failed to request magic link",
        statusCode: 500,
      });
    });
  });

  // -----------------------------------------------------------------------
  // verifyMagicLink
  // -----------------------------------------------------------------------
  describe("verifyMagicLink", () => {
    const verifyData = {
      success: true,
      token: "session-token",
      user: { id: "u1", email: "user@example.com" },
    };

    it("sends GET with token query param", async () => {
      const fetchMock = stubFetch(mockResponse(verifyData));

      const result = await service.verifyMagicLink("magic-token-123");

      expect(result).toEqual(verifyData);
      const url = fetchMock.mock.calls[0][0] as string;
      expect(url).toContain("/auth/verify");
      expect(url).toContain("token=magic-token-123");
    });

    it("includes device_name and replace_device_id when provided", async () => {
      const fetchMock = stubFetch(mockResponse(verifyData));

      await service.verifyMagicLink("tok", "My Laptop", "old-device-id");

      const url = fetchMock.mock.calls[0][0] as string;
      expect(url).toContain("device_name=My+Laptop");
      expect(url).toContain("replace_device_id=old-device-id");
    });

    it("throws AuthError with devices list on 403", async () => {
      const devices = [{ id: "d1", name: "Phone", last_seen_at: "2025-01-01" }];
      stubFetch(
        mockResponse(
          { error: "Device limit reached", devices },
          { status: 403, ok: false },
        ),
      );

      try {
        await service.verifyMagicLink("tok", "Browser");
        expect.fail("should have thrown");
      } catch (err) {
        expect(err).toBeInstanceOf(AuthError);
        const authErr = err as AuthError;
        expect(authErr.statusCode).toBe(403);
        expect(authErr.devices).toEqual(devices);
      }
    });

    it("uses fallback message when server error field is absent", async () => {
      stubFetch(mockResponse({}, { status: 500, ok: false }));

      await expect(service.verifyMagicLink("tok")).rejects.toMatchObject({
        message: "Failed to verify magic link",
      });
    });
  });

  // -----------------------------------------------------------------------
  // verifyCode
  // -----------------------------------------------------------------------
  describe("verifyCode", () => {
    const verifyData = {
      success: true,
      token: "session-token",
      user: { id: "u1", email: "user@example.com" },
    };

    it("sends POST with code, email, and optional params", async () => {
      const fetchMock = stubFetch(mockResponse(verifyData));

      const result = await service.verifyCode("123456", "user@example.com", "Laptop", "old-dev");

      expect(result).toEqual(verifyData);
      expect(fetchMock).toHaveBeenCalledWith(
        `${SERVER}/auth/verify-code`,
        expect.objectContaining({ method: "POST", credentials: "include" }),
      );
      const body = JSON.parse(fetchMock.mock.calls[0][1].body);
      expect(body).toEqual({
        code: "123456",
        email: "user@example.com",
        device_name: "Laptop",
        replace_device_id: "old-dev",
      });
    });

    it("throws AuthError with devices on failure", async () => {
      const devices = [{ id: "d1", name: null, last_seen_at: "2025-01-01" }];
      stubFetch(
        mockResponse({ error: "Device limit", devices }, { status: 403, ok: false }),
      );

      await expect(
        service.verifyCode("000000", "user@example.com"),
      ).rejects.toMatchObject({
        statusCode: 403,
        message: "Device limit",
      });
    });

    it("uses fallback message when server error field is absent", async () => {
      stubFetch(mockResponse({}, { status: 400, ok: false }));

      await expect(
        service.verifyCode("000000", "user@example.com"),
      ).rejects.toMatchObject({
        message: "Failed to verify code",
      });
    });
  });

  // -----------------------------------------------------------------------
  // getMe (getCurrentUser)
  // -----------------------------------------------------------------------
  describe("getMe", () => {
    const meData = {
      user: { id: "u1", email: "user@example.com" },
      workspaces: [{ id: "w1", name: "Default" }],
      devices: [],
      workspace_limit: 3,
      tier: "free",
      published_site_limit: 1,
      attachment_limit_bytes: 100_000_000,
    };

    it("returns user info on success", async () => {
      stubFetch(mockResponse(meData));
      const result = await service.getMe("my-token");
      expect(result).toEqual(meData);
    });

    it("sends Authorization header when token provided", async () => {
      const fetchMock = stubFetch(mockResponse(meData));
      await service.getMe("my-token");

      const callInit = fetchMock.mock.calls[0][1];
      expect(callInit.headers).toEqual(
        expect.objectContaining({ Authorization: "Bearer my-token" }),
      );
    });

    it("sends no Authorization header when token omitted", async () => {
      const fetchMock = stubFetch(mockResponse(meData));
      await service.getMe();

      const callInit = fetchMock.mock.calls[0][1];
      expect(callInit.headers).toEqual({});
    });

    it("throws 'Session expired' AuthError on 401", async () => {
      stubFetch(mockResponse({}, { status: 401, ok: false }));

      await expect(service.getMe("bad-token")).rejects.toMatchObject({
        message: "Session expired",
        statusCode: 401,
      });
    });

    it("throws generic AuthError on other failure status", async () => {
      stubFetch(mockResponse({}, { status: 500, ok: false }));

      await expect(service.getMe()).rejects.toMatchObject({
        message: "Failed to get user info",
        statusCode: 500,
      });
    });
  });

  // -----------------------------------------------------------------------
  // logout
  // -----------------------------------------------------------------------
  describe("logout", () => {
    it("sends POST to /auth/logout", async () => {
      const fetchMock = stubFetch(mockResponse({}));
      await service.logout("my-token");

      expect(fetchMock).toHaveBeenCalledWith(
        `${SERVER}/auth/logout`,
        expect.objectContaining({
          method: "POST",
          headers: { Authorization: "Bearer my-token" },
        }),
      );
    });

    it("does not throw even if response is not ok", async () => {
      stubFetch(mockResponse({}, { status: 500, ok: false }));
      // logout doesn't check response.ok
      await expect(service.logout()).resolves.toBeUndefined();
    });
  });

  // -----------------------------------------------------------------------
  // getDevices (listDevices)
  // -----------------------------------------------------------------------
  describe("getDevices", () => {
    const devices = [
      { id: "d1", name: "Laptop", last_seen_at: "2025-06-01T00:00:00Z" },
      { id: "d2", name: null, last_seen_at: "2025-06-02T00:00:00Z" },
    ];

    it("returns devices array on success", async () => {
      const fetchMock = stubFetch(mockResponse(devices));
      const result = await service.getDevices("tok");

      expect(result).toEqual(devices);
      const url = fetchMock.mock.calls[0][0] as string;
      expect(url).toBe(`${SERVER}/auth/devices`);
    });

    it("throws AuthError on failure", async () => {
      stubFetch(mockResponse({}, { status: 401, ok: false }));

      await expect(service.getDevices()).rejects.toMatchObject({
        message: "Failed to get devices",
        statusCode: 401,
      });
    });
  });

  // -----------------------------------------------------------------------
  // renameDevice
  // -----------------------------------------------------------------------
  describe("renameDevice", () => {
    it("sends PATCH with new name to /auth/devices/:id", async () => {
      const fetchMock = stubFetch(mockResponse({}));
      await service.renameDevice("tok", "device-123", "New Name");

      expect(fetchMock).toHaveBeenCalledWith(
        `${SERVER}/auth/devices/device-123`,
        expect.objectContaining({ method: "PATCH" }),
      );
      const body = JSON.parse(fetchMock.mock.calls[0][1].body);
      expect(body).toEqual({ name: "New Name" });
    });

    it("throws AuthError on failure", async () => {
      stubFetch(mockResponse({}, { status: 404, ok: false }));

      await expect(
        service.renameDevice("tok", "bad-id", "Name"),
      ).rejects.toMatchObject({
        message: "Failed to rename device",
        statusCode: 404,
      });
    });
  });

  // -----------------------------------------------------------------------
  // deleteAccount
  // -----------------------------------------------------------------------
  describe("deleteAccount", () => {
    it("sends DELETE to /auth/account", async () => {
      const fetchMock = stubFetch(mockResponse({}));
      await service.deleteAccount("tok");

      expect(fetchMock).toHaveBeenCalledWith(
        `${SERVER}/auth/account`,
        expect.objectContaining({
          method: "DELETE",
          headers: { Authorization: "Bearer tok" },
        }),
      );
    });

    it("throws AuthError on failure", async () => {
      stubFetch(mockResponse({}, { status: 500, ok: false }));

      await expect(service.deleteAccount()).rejects.toMatchObject({
        message: "Failed to delete account",
        statusCode: 500,
      });
    });
  });

  // -----------------------------------------------------------------------
  // getStatus (checkServerHealth)
  // -----------------------------------------------------------------------
  describe("getStatus", () => {
    it("returns status payload", async () => {
      const payload = { status: "ok", version: "1.0.0", active_connections: 42 };
      const fetchMock = stubFetch(mockResponse(payload));

      const result = await service.getStatus();
      expect(result).toEqual(payload);
      expect(fetchMock.mock.calls[0][0]).toBe(`${SERVER}/api/status`);
    });
  });

  // -----------------------------------------------------------------------
  // checkUserHasData (hasServerData)
  // -----------------------------------------------------------------------
  describe("checkUserHasData", () => {
    it("returns has_data payload on success", async () => {
      const payload = { has_data: true, file_count: 15 };
      stubFetch(mockResponse(payload));

      const result = await service.checkUserHasData("tok");
      expect(result).toEqual(payload);
    });

    it("returns false payload when no data", async () => {
      const payload = { has_data: false, file_count: 0 };
      stubFetch(mockResponse(payload));

      const result = await service.checkUserHasData("tok");
      expect(result).toEqual(payload);
    });

    it("throws AuthError on failure", async () => {
      stubFetch(mockResponse({}, { status: 403, ok: false }));

      await expect(service.checkUserHasData()).rejects.toMatchObject({
        message: "Failed to check user data",
        statusCode: 403,
      });
    });
  });

  // -----------------------------------------------------------------------
  // listNamespaces
  // -----------------------------------------------------------------------
  describe("listNamespaces", () => {
    it("returns namespace entries on success", async () => {
      const namespaces = [
        { id: "ns1", owner_user_id: "u1", created_at: 1700000000, metadata: { kind: "diary" } },
      ];
      const fetchMock = stubFetch(mockResponse(namespaces));

      const result = await service.listNamespaces("tok");
      expect(result).toEqual(namespaces);
      expect(fetchMock.mock.calls[0][0]).toBe(`${SERVER}/namespaces`);
    });

    it("throws AuthError on failure", async () => {
      stubFetch(mockResponse({}, { status: 401, ok: false }));

      await expect(service.listNamespaces()).rejects.toMatchObject({
        message: "Failed to list namespaces",
        statusCode: 401,
      });
    });
  });

  // -----------------------------------------------------------------------
  // Workspace CRUD
  // -----------------------------------------------------------------------
  describe("createWorkspace", () => {
    it("returns created workspace on success", async () => {
      const ws = { id: "w1", name: "My Diary" };
      const fetchMock = stubFetch(mockResponse(ws));

      const result = await service.createWorkspace("tok", "My Diary");
      expect(result).toEqual(ws);
      expect(fetchMock).toHaveBeenCalledWith(
        `${SERVER}/api/workspaces`,
        expect.objectContaining({ method: "POST" }),
      );
    });

    it("throws 403 AuthError when workspace limit reached", async () => {
      stubFetch(
        mockResponse({ error: "Workspace limit reached" }, { status: 403, ok: false }),
      );

      await expect(service.createWorkspace("tok", "Extra")).rejects.toMatchObject({
        statusCode: 403,
        message: "Workspace limit reached",
      });
    });

    it("throws 409 AuthError when name is taken", async () => {
      stubFetch(
        mockResponse({ error: "Workspace name already taken" }, { status: 409, ok: false }),
      );

      await expect(service.createWorkspace("tok", "Dupe")).rejects.toMatchObject({
        statusCode: 409,
        message: "Workspace name already taken",
      });
    });
  });

  describe("renameWorkspace", () => {
    it("sends PATCH with new name", async () => {
      const fetchMock = stubFetch(mockResponse({}));
      await service.renameWorkspace("tok", "w1", "Renamed");

      expect(fetchMock).toHaveBeenCalledWith(
        `${SERVER}/api/workspaces/w1`,
        expect.objectContaining({ method: "PATCH" }),
      );
      const body = JSON.parse(fetchMock.mock.calls[0][1].body);
      expect(body).toEqual({ name: "Renamed" });
    });

    it("throws AuthError on failure", async () => {
      stubFetch(
        mockResponse({ error: "Not found" }, { status: 404, ok: false }),
      );

      await expect(
        service.renameWorkspace("tok", "bad", "Name"),
      ).rejects.toMatchObject({
        statusCode: 404,
        message: "Not found",
      });
    });
  });

  describe("deleteWorkspace", () => {
    it("sends DELETE to /api/workspaces/:id", async () => {
      const fetchMock = stubFetch(mockResponse({}));
      await service.deleteWorkspace("tok", "w1");

      expect(fetchMock).toHaveBeenCalledWith(
        `${SERVER}/api/workspaces/w1`,
        expect.objectContaining({
          method: "DELETE",
          headers: { Authorization: "Bearer tok" },
        }),
      );
    });

    it("treats 404 as already deleted (no throw)", async () => {
      stubFetch(
        mockResponse({ error: "Workspace not found" }, { status: 404, ok: false }),
      );

      await expect(
        service.deleteWorkspace("tok", "gone"),
      ).resolves.toBeUndefined();
    });

    it("throws AuthError on other failure statuses", async () => {
      stubFetch(
        mockResponse({ error: "Server error" }, { status: 500, ok: false }),
      );

      await expect(
        service.deleteWorkspace("tok", "w1"),
      ).rejects.toMatchObject({
        statusCode: 500,
        message: "Server error",
      });
    });
  });

  // -----------------------------------------------------------------------
  // AuthError class
  // -----------------------------------------------------------------------
  describe("AuthError", () => {
    it("has correct name, message, statusCode, and details", () => {
      const err = new AuthError("Something failed", 422, { detail: "bad input" });
      expect(err).toBeInstanceOf(Error);
      expect(err.name).toBe("AuthError");
      expect(err.message).toBe("Something failed");
      expect(err.statusCode).toBe(422);
      expect(err.details).toEqual({ detail: "bad input" });
    });

    it("includes optional devices field", () => {
      const devices = [{ id: "d1", name: "Phone", last_seen_at: "2025-01-01" }];
      const err = new AuthError("limit", 403, undefined, devices);
      expect(err.devices).toEqual(devices);
    });

    it("defaults devices to undefined", () => {
      const err = new AuthError("fail", 500);
      expect(err.devices).toBeUndefined();
      expect(err.details).toBeUndefined();
    });
  });

  // -----------------------------------------------------------------------
  // Constructor: trailing slash removal
  // -----------------------------------------------------------------------
  describe("constructor", () => {
    it("strips trailing slash from server URL", async () => {
      const svc = createAuthService("http://localhost:3030/");
      const fetchMock = stubFetch(
        mockResponse({ status: "ok", version: "1.0", active_connections: 0 }),
      );

      await svc.getStatus();
      expect(fetchMock.mock.calls[0][0]).toBe("http://localhost:3030/api/status");
    });
  });

  // -----------------------------------------------------------------------
  // Existing quota / error tests (preserved)
  // -----------------------------------------------------------------------
  describe("quota errors", () => {
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

      await expect(
        service.initAttachmentUpload("token", "workspace", {
          entry_path: "notes/day.md",
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

      await expect(
        service.completeAttachmentUpload("token", "workspace", "upload-id", {
          entry_path: "notes/day.md",
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

    it("handles rate limit (429) on snapshot upload", async () => {
      vi.stubGlobal(
        "fetch",
        vi.fn().mockResolvedValue({
          ok: false,
          status: 429,
          headers: {
            get: (name: string) => {
              if (name === "content-type") return "application/json";
              if (name === "Retry-After") return "30";
              return null;
            },
          },
          json: async () => ({ error: "rate_limited" }),
        }),
      );

      await expect(
        service.uploadWorkspaceSnapshot("token", "workspace", new Blob(["x"])),
      ).rejects.toMatchObject({
        statusCode: 429,
        message: expect.stringContaining("rate limit"),
      });
    });
  });

  // -----------------------------------------------------------------------
  // deleteDevice
  // -----------------------------------------------------------------------
  describe("deleteDevice", () => {
    it("sends DELETE to /auth/devices/:id", async () => {
      const fetchMock = stubFetch(mockResponse({}));
      await service.deleteDevice("tok", "device-42");

      expect(fetchMock).toHaveBeenCalledWith(
        `${SERVER}/auth/devices/device-42`,
        expect.objectContaining({
          method: "DELETE",
          headers: { Authorization: "Bearer tok" },
        }),
      );
    });

    it("throws AuthError on failure", async () => {
      stubFetch(mockResponse({}, { status: 404, ok: false }));

      await expect(
        service.deleteDevice("tok", "bad-id"),
      ).rejects.toMatchObject({
        message: "Failed to delete device",
        statusCode: 404,
      });
    });
  });
});
