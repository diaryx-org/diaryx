import { describe, it, expect, vi, beforeEach } from "vitest";
import { AuthError } from "./authService";

// ---------------------------------------------------------------------------
// Hoisted mocks
// ---------------------------------------------------------------------------

const mockAuthService = vi.hoisted(() => ({
  // Legacy AuthService methods (still used for non-migrated endpoints).
  requestMagicLink: vi.fn(),
  verifyMagicLink: vi.fn(),
  verifyCode: vi.fn(),
  getMe: vi.fn(),
  logout: vi.fn().mockResolvedValue(undefined),
  listNamespaces: vi.fn(),
  renameDevice: vi.fn(),
  deleteDevice: vi.fn(),
  deleteAccount: vi.fn(),
  createWorkspace: vi.fn(),
  renameWorkspace: vi.fn(),
  deleteWorkspace: vi.fn(),
  createCheckoutSession: vi.fn(),
  createPortalSession: vi.fn(),
  verifyAppleTransaction: vi.fn(),
  restoreApplePurchases: vi.fn(),
  startPasskeyRegistration: vi.fn(),
  finishPasskeyRegistration: vi.fn(),
  startPasskeyAuthentication: vi.fn(),
  finishPasskeyAuthentication: vi.fn(),
  listPasskeys: vi.fn(),
  deletePasskey: vi.fn(),
  checkUserHasData: vi.fn(),
  downloadWorkspaceSnapshot: vi.fn(),
  uploadWorkspaceSnapshot: vi.fn(),
  initAttachmentUpload: vi.fn(),
  uploadAttachmentPart: vi.fn(),
  completeAttachmentUpload: vi.fn(),
  downloadAttachment: vi.fn(),
  // CoreAuthService methods (the migrated 12 + helpers).
  isAuthenticated: vi.fn().mockResolvedValue(false),
  getMetadata: vi.fn().mockResolvedValue(null),
  refreshToken: vi.fn(),
  getDevices: vi.fn().mockResolvedValue([]),
}));

const mockCreateAuthService = vi.hoisted(() => vi.fn(() => mockAuthService));
const mockSetCoreAuthServerUrl = vi.hoisted(() =>
  vi.fn().mockResolvedValue(undefined),
);

const mockCollaborationStore = vi.hoisted(() => ({
  collaborationStore: {
    setServerOffline: vi.fn(),
    setEnabled: vi.fn(),
    setSyncStatus: vi.fn(),
  },
}));

const mockProxyFetch = vi.hoisted(() => vi.fn());

const mockIsTauri = vi.hoisted(() => vi.fn().mockReturnValue(false));

const mockGetCurrentWorkspaceId = vi.hoisted(() =>
  vi.fn().mockReturnValue(null),
);

// ---------------------------------------------------------------------------
// vi.mock calls
// ---------------------------------------------------------------------------

vi.mock("./authService", async (importOriginal) => {
  const original = await importOriginal<typeof import("./authService")>();
  return {
    ...original,
    createAuthService: mockCreateAuthService,
  };
});

// The auth store routes the migrated 12 methods (verifyMagicLink, getMe,
// logout, createWorkspace, etc.) through `coreAuthService` from
// `./coreAuthRouter`, so the same mock object backs both. This avoids loading
// the real wasm/tauri implementations during tests.
vi.mock("./coreAuthRouter", () => ({
  coreAuthService: mockAuthService,
  setCoreAuthServerUrl: mockSetCoreAuthServerUrl,
}));

vi.mock("@/models/stores/collaborationStore.svelte", () => mockCollaborationStore);

vi.mock("$lib/backend/proxyFetch", () => ({
  proxyFetch: mockProxyFetch,
}));

vi.mock("$lib/backend/interface", () => ({
  isTauri: mockIsTauri,
}));

vi.mock("$lib/storage/localWorkspaceRegistry.svelte", () => ({
  getCurrentWorkspaceId: mockGetCurrentWorkspaceId,
}));

vi.mock("./webauthnUtils", () => ({
  prepareCreationOptions: vi.fn(),
  prepareRequestOptions: vi.fn(),
  serializeRegistrationCredential: vi.fn(),
  serializeAuthenticationCredential: vi.fn(),
}));

vi.mock("$lib/credentials", () => ({
  getCredential: vi.fn().mockResolvedValue(null),
  storeCredential: vi.fn().mockResolvedValue(undefined),
  removeCredential: vi.fn().mockResolvedValue(undefined),
}));

// ---------------------------------------------------------------------------
// Import the module under test (after mocks are set up)
// ---------------------------------------------------------------------------

import {
  initAuth,
  setServerUrl,
  requestMagicLink,
  verifyMagicLink,
  verifyCode,
  logout,
  refreshUserInfo,
  isAuthenticated,
  getUser,
  getServerUrl,
  getAuthState,
  getDeviceReplacementContext,
  clearDeviceReplacement,
  retryWithDeviceReplacement,
  checkServerHealth,
  refreshUserStorageUsage,
  listUserWorkspaceNamespaces,
  getDefaultWorkspace,
  getCurrentWorkspace,
  setActiveWorkspaceId,
  getWorkspaces,
  getWorkspaceLimit,
  getStorageUsage,
  getToken,
  enableSync,
  isSyncEnabled,
} from "./authStore.svelte";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeMeResponse(overrides: Record<string, unknown> = {}) {
  return {
    user: { id: "u1", email: "test@example.com" },
    workspaces: [{ id: "ws1", name: "My Workspace" }],
    devices: [{ id: "d1", name: "Chrome", last_seen_at: "2024-01-01" }],
    workspace_limit: 3,
    tier: "plus",
    published_site_limit: 5,
    attachment_limit_bytes: 500 * 1024 * 1024,
    ...overrides,
  };
}

function makeVerifyResponse(overrides: Record<string, unknown> = {}) {
  return {
    success: true,
    token: "tok_abc",
    user: { id: "u1", email: "test@example.com" },
    ...overrides,
  };
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("authStore", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
    mockIsTauri.mockReturnValue(false);
    mockCreateAuthService.mockReturnValue(mockAuthService);
    // Default safe return values for commonly called methods
    mockAuthService.listNamespaces.mockResolvedValue([]);
    mockAuthService.logout.mockResolvedValue(undefined);

    // Reset module-level state
    setServerUrl(null);
    clearDeviceReplacement();
  });

  // =========================================================================
  // Getters (initial state)
  // =========================================================================

  describe("getters", () => {
    it("isAuthenticated returns false initially", () => {
      expect(isAuthenticated()).toBe(false);
    });

    it("getUser returns null initially", () => {
      expect(getUser()).toBeNull();
    });

    it("getServerUrl returns null initially", () => {
      expect(getServerUrl()).toBeNull();
    });

    it("getToken always returns null (cookie-based auth)", () => {
      expect(getToken()).toBeNull();
    });

    it("getAuthState returns the full state object", () => {
      const state = getAuthState();
      expect(state).toHaveProperty("isAuthenticated");
      expect(state).toHaveProperty("user");
      expect(state).toHaveProperty("serverUrl");
      expect(state).toHaveProperty("workspaces");
      expect(state).toHaveProperty("devices");
    });

    it("getDefaultWorkspace returns null when no workspaces", () => {
      expect(getDefaultWorkspace()).toBeNull();
    });

    it("getWorkspaces returns empty array initially", () => {
      expect(getWorkspaces()).toEqual([]);
    });

    it("getWorkspaceLimit returns default of 1", () => {
      expect(getWorkspaceLimit()).toBe(1);
    });

    it("getStorageUsage returns null initially", () => {
      expect(getStorageUsage()).toBeNull();
    });
  });

  // =========================================================================
  // setServerUrl
  // =========================================================================

  describe("setServerUrl", () => {
    it("saves URL to localStorage and creates authService", () => {
      setServerUrl("https://sync.example.com");

      expect(getServerUrl()).toBe("https://sync.example.com");
      expect(localStorage.setItem).toHaveBeenCalledWith(
        "diaryx_sync_server_url",
        "https://sync.example.com",
      );
      expect(mockCreateAuthService).toHaveBeenCalledWith(
        "https://sync.example.com",
      );
    });

    it("clears state when set to null", () => {
      setServerUrl("https://sync.example.com");
      setServerUrl(null);

      expect(getServerUrl()).toBeNull();
      expect(localStorage.removeItem).toHaveBeenCalledWith(
        "diaryx_sync_server_url",
      );
    });
  });

  // =========================================================================
  // checkServerHealth
  // =========================================================================

  describe("checkServerHealth", () => {
    it("returns true when server responds ok", async () => {
      mockProxyFetch.mockResolvedValue({ ok: true });

      const result = await checkServerHealth("https://sync.example.com");
      expect(result).toBe(true);
      expect(mockProxyFetch).toHaveBeenCalledWith(
        "https://sync.example.com/api/health",
        { method: "GET", timeout_ms: 5000 },
      );
    });

    it("returns false when server responds not ok", async () => {
      mockProxyFetch.mockResolvedValue({ ok: false });

      const result = await checkServerHealth("https://sync.example.com");
      expect(result).toBe(false);
    });

    it("returns false when fetch throws", async () => {
      mockProxyFetch.mockRejectedValue(new Error("Network error"));

      const result = await checkServerHealth("https://sync.example.com");
      expect(result).toBe(false);
    });

    it("handles URL that already ends with /api", async () => {
      mockProxyFetch.mockResolvedValue({ ok: true });

      await checkServerHealth("https://sync.example.com/api");
      expect(mockProxyFetch).toHaveBeenCalledWith(
        "https://sync.example.com/api/health",
        expect.any(Object),
      );
    });

    it("strips trailing slashes from URL", async () => {
      mockProxyFetch.mockResolvedValue({ ok: true });

      await checkServerHealth("https://sync.example.com/");
      expect(mockProxyFetch).toHaveBeenCalledWith(
        "https://sync.example.com/api/health",
        expect.any(Object),
      );
    });
  });

  // =========================================================================
  // initAuth
  // =========================================================================

  describe("initAuth", () => {
    it("does nothing when localStorage is undefined", async () => {
      // Save original
      const origLocalStorage = globalThis.localStorage;
      // @ts-expect-error - simulating no localStorage
      delete globalThis.localStorage;

      // Should not throw
      // We need to re-import or test differently. Since localStorage is
      // checked at runtime, we can monkey-patch:
      Object.defineProperty(globalThis, "localStorage", {
        value: undefined,
        configurable: true,
        writable: true,
      });

      await initAuth();
      // No calls should have been made
      expect(mockCreateAuthService).not.toHaveBeenCalled();

      // Restore
      Object.defineProperty(globalThis, "localStorage", {
        value: origLocalStorage,
        configurable: true,
        writable: true,
      });
    });

    it("restores serverUrl from localStorage and creates authService", async () => {
      localStorage.setItem("diaryx_sync_server_url", "https://sync.example.com");
      // Health check passes, getMe succeeds
      mockProxyFetch.mockResolvedValue({ ok: true });
      mockAuthService.getMe.mockResolvedValue(makeMeResponse());

      await initAuth();

      expect(getServerUrl()).toBe("https://sync.example.com");
      expect(mockCreateAuthService).toHaveBeenCalledWith(
        "https://sync.example.com",
      );
    });

    it("enters offline mode when server is unreachable", async () => {
      localStorage.setItem("diaryx_sync_server_url", "https://sync.example.com");
      localStorage.setItem(
        "diaryx_user",
        JSON.stringify({ id: "u1", email: "test@example.com" }),
      );
      mockProxyFetch.mockResolvedValue({ ok: false });

      await initAuth();

      expect(
        mockCollaborationStore.collaborationStore.setServerOffline,
      ).toHaveBeenCalledWith(true);
      // Should still restore cached user
      expect(isAuthenticated()).toBe(true);
      expect(getUser()).toEqual({ id: "u1", email: "test@example.com" });
    });

    it("validates session and populates state on successful getMe", async () => {
      localStorage.setItem("diaryx_sync_server_url", "https://sync.example.com");
      mockProxyFetch.mockResolvedValue({ ok: true });
      const meResponse = makeMeResponse();
      mockAuthService.getMe.mockResolvedValue(meResponse);

      await initAuth();

      expect(isAuthenticated()).toBe(true);
      expect(getUser()).toEqual(meResponse.user);
      expect(getWorkspaces()).toEqual(meResponse.workspaces);
      expect(getWorkspaceLimit()).toBe(3);
      expect(getAuthState().tier).toBe("plus");
      // Should clear offline state
      expect(
        mockCollaborationStore.collaborationStore.setServerOffline,
      ).toHaveBeenCalledWith(false);
    });

    it("clears auth on 401 from getMe", async () => {
      localStorage.setItem("diaryx_sync_server_url", "https://sync.example.com");
      mockProxyFetch.mockResolvedValue({ ok: true });
      mockAuthService.getMe.mockRejectedValue(
        new AuthError("Unauthorized", 401),
      );
      mockAuthService.logout.mockResolvedValue(undefined);

      await initAuth();

      // logout should have been called, clearing state
      expect(isAuthenticated()).toBe(false);
      expect(getUser()).toBeNull();
    });

    it("keeps cached user on non-401 errors", async () => {
      localStorage.setItem("diaryx_sync_server_url", "https://sync.example.com");
      localStorage.setItem(
        "diaryx_user",
        JSON.stringify({ id: "u1", email: "cached@example.com" }),
      );
      mockProxyFetch.mockResolvedValue({ ok: true });
      mockAuthService.getMe.mockRejectedValue(new Error("Network timeout"));

      await initAuth();

      expect(isAuthenticated()).toBe(true);
      expect(getUser()?.email).toBe("cached@example.com");
    });

    it("re-enables sync when diaryx_sync_enabled was true", async () => {
      localStorage.setItem("diaryx_sync_server_url", "https://sync.example.com");
      localStorage.setItem("diaryx_sync_enabled", "true");
      mockProxyFetch.mockResolvedValue({ ok: true });
      mockAuthService.getMe.mockResolvedValue(makeMeResponse());

      await initAuth();

      expect(
        mockCollaborationStore.collaborationStore.setEnabled,
      ).toHaveBeenCalledWith(true);
      expect(
        mockCollaborationStore.collaborationStore.setSyncStatus,
      ).toHaveBeenCalledWith("idle");
    });

    it("does not enable sync when diaryx_sync_enabled is absent", async () => {
      localStorage.setItem("diaryx_sync_server_url", "https://sync.example.com");
      mockProxyFetch.mockResolvedValue({ ok: true });
      mockAuthService.getMe.mockResolvedValue(makeMeResponse());

      await initAuth();

      expect(
        mockCollaborationStore.collaborationStore.setEnabled,
      ).not.toHaveBeenCalled();
    });
  });

  // =========================================================================
  // requestMagicLink
  // =========================================================================

  describe("requestMagicLink", () => {
    it("throws when server URL not configured", async () => {
      await expect(requestMagicLink("test@example.com")).rejects.toThrow(
        "Server URL not configured",
      );
    });

    it("returns success with dev link on success", async () => {
      setServerUrl("https://sync.example.com");
      mockAuthService.requestMagicLink.mockResolvedValue({
        success: true,
        message: "Sent",
        dev_link: "http://localhost/verify?token=abc",
        dev_code: "123456",
      });

      const result = await requestMagicLink("test@example.com");

      expect(result).toEqual({
        success: true,
        devLink: "http://localhost/verify?token=abc",
        devCode: "123456",
      });
      expect(mockAuthService.requestMagicLink).toHaveBeenCalledWith(
        "test@example.com",
      );
    });

    it("sets error state and re-throws on failure", async () => {
      setServerUrl("https://sync.example.com");
      mockAuthService.requestMagicLink.mockRejectedValue(
        new Error("Rate limited"),
      );

      await expect(requestMagicLink("test@example.com")).rejects.toThrow(
        "Rate limited",
      );
      expect(getAuthState().error).toBe("Rate limited");
      expect(getAuthState().isLoading).toBe(false);
    });
  });

  // =========================================================================
  // verifyMagicLink
  // =========================================================================

  describe("verifyMagicLink", () => {
    it("throws when server URL not configured", async () => {
      await expect(verifyMagicLink("tok_abc")).rejects.toThrow(
        "Server URL not configured",
      );
    });

    it("authenticates user on success", async () => {
      setServerUrl("https://sync.example.com");
      mockAuthService.verifyMagicLink.mockResolvedValue(makeVerifyResponse());
      mockAuthService.getMe.mockResolvedValue(makeMeResponse());

      await verifyMagicLink("tok_abc", "My Device");

      expect(isAuthenticated()).toBe(true);
      expect(getUser()).toEqual({ id: "u1", email: "test@example.com" });
      expect(mockAuthService.verifyMagicLink).toHaveBeenCalledWith(
        "tok_abc",
        "My Device",
        undefined,
      );
    });

    it("passes replaceDeviceId when provided", async () => {
      setServerUrl("https://sync.example.com");
      mockAuthService.verifyMagicLink.mockResolvedValue(makeVerifyResponse());
      mockAuthService.getMe.mockResolvedValue(makeMeResponse());

      await verifyMagicLink("tok_abc", "My Device", "old-device-id");

      expect(mockAuthService.verifyMagicLink).toHaveBeenCalledWith(
        "tok_abc",
        "My Device",
        "old-device-id",
      );
    });

    it("stores device replacement context on 403 with devices", async () => {
      setServerUrl("https://sync.example.com");
      const devices = [
        { id: "d1", name: "Old Device", last_seen_at: "2024-01-01" },
        { id: "d2", name: "Other Device", last_seen_at: "2024-06-01" },
      ];
      const err = new AuthError("Device limit", 403, undefined, devices);
      mockAuthService.verifyMagicLink.mockRejectedValue(err);

      await expect(verifyMagicLink("tok_abc")).rejects.toThrow("Device limit");

      const ctx = getDeviceReplacementContext();
      expect(ctx).not.toBeNull();
      expect(ctx!.kind).toBe("magic_link");
      expect(ctx!.devices).toEqual(devices);
      expect(ctx!.args.token).toBe("tok_abc");
    });

    it("sets error state on failure", async () => {
      setServerUrl("https://sync.example.com");
      mockAuthService.verifyMagicLink.mockRejectedValue(
        new Error("Invalid token"),
      );

      await expect(verifyMagicLink("bad_token")).rejects.toThrow(
        "Invalid token",
      );
      expect(getAuthState().error).toBe("Invalid token");
      expect(getAuthState().isLoading).toBe(false);
    });
  });

  // =========================================================================
  // verifyCode
  // =========================================================================

  describe("verifyCode", () => {
    it("throws when server URL not configured", async () => {
      await expect(
        verifyCode("123456", "test@example.com"),
      ).rejects.toThrow("Server URL not configured");
    });

    it("authenticates user on success", async () => {
      setServerUrl("https://sync.example.com");
      mockAuthService.verifyCode.mockResolvedValue(makeVerifyResponse());
      mockAuthService.getMe.mockResolvedValue(makeMeResponse());

      await verifyCode("123456", "test@example.com", "My Device");

      expect(isAuthenticated()).toBe(true);
      expect(mockAuthService.verifyCode).toHaveBeenCalledWith(
        "123456",
        "test@example.com",
        "My Device",
        undefined,
      );
    });

    it("stores device replacement context on 403 with devices", async () => {
      setServerUrl("https://sync.example.com");
      const devices = [
        { id: "d1", name: "Device A", last_seen_at: "2024-01-01" },
      ];
      const err = new AuthError("Device limit", 403, undefined, devices);
      mockAuthService.verifyCode.mockRejectedValue(err);

      await expect(
        verifyCode("123456", "test@example.com"),
      ).rejects.toThrow("Device limit");

      const ctx = getDeviceReplacementContext();
      expect(ctx).not.toBeNull();
      expect(ctx!.kind).toBe("code");
      expect(ctx!.args.code).toBe("123456");
      expect(ctx!.args.email).toBe("test@example.com");
    });
  });

  // =========================================================================
  // logout
  // =========================================================================

  describe("logout", () => {
    it("clears auth state", async () => {
      // Set up authenticated state first
      setServerUrl("https://sync.example.com");
      mockAuthService.verifyMagicLink.mockResolvedValue(makeVerifyResponse());
      mockAuthService.getMe.mockResolvedValue(makeMeResponse());
      await verifyMagicLink("tok_abc");

      expect(isAuthenticated()).toBe(true);

      mockAuthService.logout.mockResolvedValue(undefined);
      await logout();

      expect(isAuthenticated()).toBe(false);
      expect(getUser()).toBeNull();
      expect(getWorkspaces()).toEqual([]);
      expect(getAuthState().devices).toEqual([]);
      expect(getAuthState().error).toBeNull();
    });

    it("removes user from localStorage", async () => {
      setServerUrl("https://sync.example.com");
      mockAuthService.verifyMagicLink.mockResolvedValue(makeVerifyResponse());
      mockAuthService.getMe.mockResolvedValue(makeMeResponse());
      await verifyMagicLink("tok_abc");

      await logout();

      expect(localStorage.removeItem).toHaveBeenCalledWith("diaryx_user");
      expect(localStorage.removeItem).toHaveBeenCalledWith("diaryx_sync_enabled");
    });

    it("disables collaboration", async () => {
      setServerUrl("https://sync.example.com");
      mockAuthService.logout.mockResolvedValue(undefined);

      await logout();

      expect(
        mockCollaborationStore.collaborationStore.setEnabled,
      ).toHaveBeenCalledWith(false);
    });

    it("calls server logout (fire-and-forget)", async () => {
      setServerUrl("https://sync.example.com");
      mockAuthService.logout.mockResolvedValue(undefined);

      await logout();

      expect(mockAuthService.logout).toHaveBeenCalled();
    });
  });

  // =========================================================================
  // refreshUserInfo
  // =========================================================================

  describe("refreshUserInfo", () => {
    it("does nothing when authService is null", async () => {
      // No server URL set, so authService is null
      await refreshUserInfo();
      expect(mockAuthService.getMe).not.toHaveBeenCalled();
    });

    it("updates state with fresh user info", async () => {
      setServerUrl("https://sync.example.com");
      const meResponse = makeMeResponse({
        tier: "enterprise",
        workspace_limit: 10,
      });
      mockAuthService.getMe.mockResolvedValue(meResponse);

      await refreshUserInfo();

      expect(getUser()).toEqual(meResponse.user);
      expect(getWorkspaces()).toEqual(meResponse.workspaces);
      expect(getWorkspaceLimit()).toBe(10);
      expect(getAuthState().tier).toBe("enterprise");
    });

    it("does not throw on error, just logs", async () => {
      setServerUrl("https://sync.example.com");
      mockAuthService.getMe.mockRejectedValue(new Error("Server error"));

      // Should not throw
      await expect(refreshUserInfo()).resolves.toBeUndefined();
    });
  });

  // =========================================================================
  // refreshUserStorageUsage
  // =========================================================================

  describe("refreshUserStorageUsage", () => {
    it("sets storageUsage to null (no-op stub)", async () => {
      await refreshUserStorageUsage();
      expect(getStorageUsage()).toBeNull();
    });
  });

  // =========================================================================
  // Device Replacement
  // =========================================================================

  describe("device replacement", () => {
    it("getDeviceReplacementContext returns null initially", () => {
      expect(getDeviceReplacementContext()).toBeNull();
    });

    it("clearDeviceReplacement clears the context", async () => {
      // Trigger device replacement
      setServerUrl("https://sync.example.com");
      const devices = [
        { id: "d1", name: "Device", last_seen_at: "2024-01-01" },
      ];
      const err = new AuthError("Device limit", 403, undefined, devices);
      mockAuthService.verifyMagicLink.mockRejectedValue(err);

      await expect(verifyMagicLink("tok_abc")).rejects.toThrow();

      expect(getDeviceReplacementContext()).not.toBeNull();

      clearDeviceReplacement();

      expect(getDeviceReplacementContext()).toBeNull();
    });

    it("retryWithDeviceReplacement throws if no pending context", async () => {
      await expect(
        retryWithDeviceReplacement("device-id"),
      ).rejects.toThrow("No pending device replacement");
    });

    it("retryWithDeviceReplacement retries magic_link flow", async () => {
      setServerUrl("https://sync.example.com");

      // First attempt: trigger device replacement
      const devices = [
        { id: "d1", name: "Old", last_seen_at: "2024-01-01" },
      ];
      const err = new AuthError("Device limit", 403, undefined, devices);
      mockAuthService.verifyMagicLink.mockRejectedValueOnce(err);

      await expect(verifyMagicLink("tok_abc", "MyDevice")).rejects.toThrow();

      // Now retry with device replacement
      mockAuthService.verifyMagicLink.mockResolvedValue(makeVerifyResponse());
      mockAuthService.getMe.mockResolvedValue(makeMeResponse());

      await retryWithDeviceReplacement("d1");

      expect(mockAuthService.verifyMagicLink).toHaveBeenLastCalledWith(
        "tok_abc",
        "MyDevice",
        "d1",
      );
      expect(isAuthenticated()).toBe(true);
      expect(getDeviceReplacementContext()).toBeNull();
    });

    it("retryWithDeviceReplacement retries code flow", async () => {
      setServerUrl("https://sync.example.com");

      const devices = [
        { id: "d1", name: "Old", last_seen_at: "2024-01-01" },
      ];
      const err = new AuthError("Device limit", 403, undefined, devices);
      mockAuthService.verifyCode.mockRejectedValueOnce(err);

      await expect(
        verifyCode("123456", "test@example.com", "MyDevice"),
      ).rejects.toThrow();

      mockAuthService.verifyCode.mockResolvedValue(makeVerifyResponse());
      mockAuthService.getMe.mockResolvedValue(makeMeResponse());

      await retryWithDeviceReplacement("d1");

      expect(mockAuthService.verifyCode).toHaveBeenLastCalledWith(
        "123456",
        "test@example.com",
        "MyDevice",
        "d1",
      );
      expect(isAuthenticated()).toBe(true);
    });
  });

  // =========================================================================
  // listUserWorkspaceNamespaces
  // =========================================================================

  describe("listUserWorkspaceNamespaces", () => {
    it("returns empty array when not authenticated", async () => {
      setServerUrl("https://sync.example.com");
      // isAuthenticated is false
      const result = await listUserWorkspaceNamespaces();
      expect(result).toEqual([]);
    });

    it("filters namespaces to workspace metadata type", async () => {
      setServerUrl("https://sync.example.com");
      // Make the user authenticated
      mockAuthService.verifyMagicLink.mockResolvedValue(makeVerifyResponse());
      mockAuthService.getMe.mockResolvedValue(makeMeResponse());
      await verifyMagicLink("tok_abc");

      mockAuthService.listNamespaces.mockResolvedValue([
        {
          id: "ns1",
          owner_user_id: "u1",
          created_at: 1000,
          metadata: { type: "workspace", name: "WS 1" },
        },
        {
          id: "ns2",
          owner_user_id: "u1",
          created_at: 2000,
          metadata: { type: "site", kind: "workspace", name: "Site 1" },
        },
        {
          id: "ns3",
          owner_user_id: "u1",
          created_at: 3000,
          metadata: { kind: "workspace", name: "WS legacy" },
        },
        {
          id: "ns4",
          owner_user_id: "u1",
          created_at: 4000,
          metadata: { kind: "workspace", name: "WS 2" },
        },
      ]);

      const result = await listUserWorkspaceNamespaces();

      expect(result).toHaveLength(3);
      expect(result[0].id).toBe("ns1");
      expect(result[1].id).toBe("ns3");
      expect(result[2].id).toBe("ns4");
    });

    it("returns empty array on error", async () => {
      setServerUrl("https://sync.example.com");
      mockAuthService.verifyMagicLink.mockResolvedValue(makeVerifyResponse());
      mockAuthService.getMe.mockResolvedValue(makeMeResponse());
      await verifyMagicLink("tok_abc");

      mockAuthService.listNamespaces.mockRejectedValue(
        new Error("Server error"),
      );

      const result = await listUserWorkspaceNamespaces();
      expect(result).toEqual([]);
    });
  });

  // =========================================================================
  // Workspace getters
  // =========================================================================

  describe("workspace getters", () => {
    beforeEach(async () => {
      setServerUrl("https://sync.example.com");
      mockAuthService.verifyMagicLink.mockResolvedValue(makeVerifyResponse());
      mockAuthService.getMe.mockResolvedValue(
        makeMeResponse({
          workspaces: [
            { id: "ws1", name: "Workspace 1" },
            { id: "ws2", name: "Workspace 2" },
          ],
        }),
      );
      await verifyMagicLink("tok_abc");
    });

    it("getDefaultWorkspace returns first workspace when no active ID set", () => {
      setActiveWorkspaceId(null);
      const ws = getDefaultWorkspace();
      expect(ws?.id).toBe("ws1");
    });

    it("getDefaultWorkspace returns active workspace when set", () => {
      setActiveWorkspaceId("ws2");
      const ws = getDefaultWorkspace();
      expect(ws?.id).toBe("ws2");
    });

    it("getDefaultWorkspace returns null when active ID is not in server list (local-only)", () => {
      setActiveWorkspaceId("local-ws-not-on-server");
      const ws = getDefaultWorkspace();
      expect(ws).toBeNull();
    });

    it("getCurrentWorkspace returns matching workspace for active ID", () => {
      setActiveWorkspaceId("ws2");
      const ws = getCurrentWorkspace();
      expect(ws?.id).toBe("ws2");
    });

    it("getCurrentWorkspace falls back to getDefaultWorkspace", () => {
      setActiveWorkspaceId(null);
      const ws = getCurrentWorkspace();
      expect(ws?.id).toBe("ws1");
    });
  });

  // =========================================================================
  // enableSync / isSyncEnabled
  // =========================================================================

  describe("enableSync / isSyncEnabled", () => {
    it("enableSync sets localStorage flag and enables collaboration", () => {
      enableSync();

      expect(localStorage.setItem).toHaveBeenCalledWith(
        "diaryx_sync_enabled",
        "true",
      );
      expect(
        mockCollaborationStore.collaborationStore.setEnabled,
      ).toHaveBeenCalledWith(true);
    });

    it("isSyncEnabled returns true when flag is set", () => {
      localStorage.setItem("diaryx_sync_enabled", "true");
      expect(isSyncEnabled()).toBe(true);
    });

    it("isSyncEnabled returns false when flag is absent", () => {
      expect(isSyncEnabled()).toBe(false);
    });
  });
});
