/**
 * Auth Store - Svelte 5 reactive state for authentication.
 *
 * Manages:
 * - Authentication state (logged in/out)
 * - Session token storage
 * - User info
 * - Auto-connect to sync server when logged in
 */

import {
  AuthService,
  createAuthService,
  type User,
  type Workspace,
  type Device,
  AuthError,
  type UserHasDataResponse,
  type UserStorageUsageResponse,
  type InitAttachmentUploadRequest,
  type InitAttachmentUploadResponse,
  type CompleteAttachmentUploadRequest,
  type CompleteAttachmentUploadResponse,
  type DownloadAttachmentResponse,
} from "./authService";
import {
  setAuthToken,
  setCollaborationWorkspaceId,
} from "../crdt";
import { collaborationStore } from "@/models/stores/collaborationStore.svelte";
import { getCurrentWorkspaceId as registryGetCurrentWorkspaceId } from "$lib/storage/localWorkspaceRegistry";

// ============================================================================
// Types
// ============================================================================

export interface AuthState {
  isAuthenticated: boolean;
  isLoading: boolean;
  user: User | null;
  workspaces: Workspace[];
  devices: Device[];
  workspaceLimit: number;
  tier: string;
  publishedSiteLimit: number;
  attachmentLimitBytes: number;
  /** The currently active workspace ID (reactive). Updated on switch. */
  activeWorkspaceId: string | null;
  storageUsage: UserStorageUsageResponse | null;
  error: string | null;
  serverUrl: string | null;
}

// ============================================================================
// Storage Keys
// ============================================================================

const STORAGE_KEYS = {
  TOKEN: "diaryx_auth_token",
  SERVER_URL: "diaryx_sync_server_url",
  USER: "diaryx_user",
} as const;

// ============================================================================
// State
// ============================================================================

let state = $state<AuthState>({
  isAuthenticated: false,
  isLoading: false,
  user: null,
  workspaces: [],
  devices: [],
  workspaceLimit: 1,
  tier: "free",
  publishedSiteLimit: 1,
  attachmentLimitBytes: 200 * 1024 * 1024,
  activeWorkspaceId: registryGetCurrentWorkspaceId(),
  storageUsage: null,
  error: null,
  serverUrl: null,
});

let authService: AuthService | null = null;

// ============================================================================
// Getters
// ============================================================================

export function getAuthState(): AuthState {
  return state;
}

export function isAuthenticated(): boolean {
  return state.isAuthenticated;
}

export function getUser(): User | null {
  return state.user;
}

export function getToken(): string | null {
  if (typeof localStorage === "undefined") return null;
  return localStorage.getItem(STORAGE_KEYS.TOKEN);
}

export function getServerUrl(): string | null {
  return state.serverUrl;
}

export function getDefaultWorkspace(): Workspace | null {
  return (
    state.workspaces.find((w) => w.name === "default") ??
    state.workspaces[0] ??
    null
  );
}

/**
 * Get the currently active workspace, respecting the user's workspace selection.
 *
 * Uses the reactive `activeWorkspaceId` from auth state (updated via
 * `setActiveWorkspaceId()`), then falls back to getDefaultWorkspace()
 * for backward compatibility.
 */
export function getCurrentWorkspace(): Workspace | null {
  const currentId = state.activeWorkspaceId;
  if (currentId) {
    const ws = state.workspaces.find(w => w.id === currentId);
    if (ws) return ws;
  }
  return getDefaultWorkspace();
}

/**
 * Set the active workspace ID in reactive state.
 * Call this after switching workspaces so that `$derived(getCurrentWorkspace())`
 * re-evaluates in all consumers.
 */
export function setActiveWorkspaceId(id: string | null): void {
  state.activeWorkspaceId = id;
}

/**
 * Get the list of workspaces the server knows about.
 */
export function getWorkspaces(): Workspace[] {
  return state.workspaces;
}

/**
 * Get the workspace limit for the current user.
 * Returns null if not authenticated or limit info not available.
 */
export function getWorkspaceLimit(): number {
  return state.workspaceLimit;
}

export function getStorageUsage(): UserStorageUsageResponse | null {
  return state.storageUsage;
}

// ============================================================================
// Actions
// ============================================================================

/**
 * Initialize auth state from localStorage.
 */
export async function initAuth(): Promise<void> {
  if (typeof localStorage === "undefined") return;

  const serverUrl = localStorage.getItem(STORAGE_KEYS.SERVER_URL);
  const token = localStorage.getItem(STORAGE_KEYS.TOKEN);
  const savedUser = localStorage.getItem(STORAGE_KEYS.USER);

  if (serverUrl) {
    state.serverUrl = serverUrl;
    authService = createAuthService(serverUrl);
    // Note: We do NOT call setCollaborationServer() here.
    // Sync should only start after token validation succeeds below.
  }

  if (token && serverUrl) {
    state.isLoading = true;
    state.error = null;

    // Restore user from localStorage immediately for faster UI
    if (savedUser) {
      try {
        state.user = JSON.parse(savedUser);
        state.isAuthenticated = true;
      } catch {
        // Invalid saved user
      }
    }

    try {
      // Validate token with server
      const me = await authService!.getMe(token);
      state.user = me.user;
      state.workspaces = me.workspaces;
      state.devices = me.devices;
      state.workspaceLimit = me.workspace_limit;
      state.tier = me.tier;
      state.publishedSiteLimit = me.published_site_limit;
      state.attachmentLimitBytes = me.attachment_limit_bytes;
      state.isAuthenticated = true;

      // Update collaboration settings
      setAuthToken(token);
      const activeWorkspace = getCurrentWorkspace();
      if (activeWorkspace) {
        setCollaborationWorkspaceId(activeWorkspace.id);
      }

      // Only re-enable sync if user previously completed sync setup
      // (not just authenticated). This decouples sign-in from sync.
      const syncWasEnabled = localStorage.getItem('diaryx_sync_enabled') === 'true';
      if (syncWasEnabled) {
        collaborationStore.setEnabled(true);
        collaborationStore.setSyncStatus("idle");
      }

      // Save user for faster restore next time
      localStorage.setItem(STORAGE_KEYS.USER, JSON.stringify(me.user));
      await refreshUserStorageUsage();
    } catch (err) {
      if (err instanceof AuthError && err.statusCode === 401) {
        // Token expired, clear auth state
        await logout();
      } else {
        // Network error - keep user logged in with cached data
        console.warn("[AuthStore] Failed to validate token:", err);
        if (savedUser) {
          state.isAuthenticated = true;
          const syncWasEnabled = localStorage.getItem('diaryx_sync_enabled') === 'true';
          if (syncWasEnabled) {
            collaborationStore.setEnabled(true);
            collaborationStore.setSyncStatus("idle");
          }
        }
      }
    } finally {
      state.isLoading = false;
    }
  }
}

/**
 * Set the sync server URL.
 *
 * Note: This only saves the URL - it does NOT start sync.
 * Sync is started by setWorkspaceServer() which is called
 * from SyncSetupWizard after authentication completes.
 */
export function setServerUrl(url: string | null): void {
  state.serverUrl = url;

  if (url) {
    localStorage.setItem(STORAGE_KEYS.SERVER_URL, url);
    authService = createAuthService(url);
    // Note: We intentionally do NOT call setCollaborationServer() here.
    // Sync should only start after authentication completes and user
    // chooses to sync via SyncSetupWizard.
  } else {
    localStorage.removeItem(STORAGE_KEYS.SERVER_URL);
    authService = null;
    state.storageUsage = null;
  }
}

/**
 * Request a magic link.
 */
export async function requestMagicLink(
  email: string,
): Promise<{ success: boolean; devLink?: string }> {
  if (!authService) {
    throw new Error("Server URL not configured");
  }

  state.isLoading = true;
  state.error = null;

  try {
    const response = await authService.requestMagicLink(email);
    return { success: true, devLink: response.dev_link };
  } catch (err) {
    const message =
      err instanceof Error ? err.message : "Failed to send magic link";
    state.error = message;
    throw err;
  } finally {
    state.isLoading = false;
  }
}

/**
 * Verify a magic link token and log in.
 * @param token - The magic link token
 * @param customDeviceName - Optional custom device name (from user input). Falls back to auto-detected name.
 */
export async function verifyMagicLink(token: string, customDeviceName?: string): Promise<void> {
  if (!authService) {
    throw new Error("Server URL not configured");
  }

  state.isLoading = true;
  state.error = null;

  try {
    // Use custom name if provided, otherwise auto-detect
    const deviceName = customDeviceName?.trim() || getDeviceName();

    const response = await authService.verifyMagicLink(token, deviceName);

    // Store token
    localStorage.setItem(STORAGE_KEYS.TOKEN, response.token);
    localStorage.setItem(STORAGE_KEYS.USER, JSON.stringify(response.user));

    // Update state
    state.user = response.user;
    state.isAuthenticated = true;

    // Update collaboration settings (token is set for API calls,
    // but sync is NOT auto-enabled — user must complete sync setup separately)
    setAuthToken(response.token);

    // Fetch full user info (workspaces, devices)
    await refreshUserInfo();
    await refreshUserStorageUsage();
  } catch (err) {
    const message =
      err instanceof Error ? err.message : "Failed to verify magic link";
    state.error = message;
    throw err;
  } finally {
    state.isLoading = false;
  }
}

/**
 * Refresh user info from server.
 */
export async function refreshUserInfo(): Promise<void> {
  const token = getToken();
  if (!authService || !token) return;

  try {
    const me = await authService.getMe(token);
    state.user = me.user;
    state.workspaces = me.workspaces;
    state.devices = me.devices;
    state.workspaceLimit = me.workspace_limit;
    state.tier = me.tier;
    state.publishedSiteLimit = me.published_site_limit;
    state.attachmentLimitBytes = me.attachment_limit_bytes;

    // Update workspace ID
    const activeWorkspace = getCurrentWorkspace();
    if (activeWorkspace) {
      setCollaborationWorkspaceId(activeWorkspace.id);
    }
  } catch (err) {
    console.error("[AuthStore] Failed to refresh user info:", err);
  }
}

/**
 * Refresh attachment storage usage from server.
 */
export async function refreshUserStorageUsage(): Promise<void> {
  const token = getToken();
  const url = state.serverUrl;
  if (!token || !url || !authService || !state.isAuthenticated) {
    state.storageUsage = null;
    return;
  }

  try {
    state.storageUsage = await authService.getUserStorageUsage(token);
  } catch (err) {
    console.error("[AuthStore] Failed to refresh storage usage:", err);
  }
}

/**
 * Explicitly enable sync. Called by SyncSetupWizard after workspace initialization.
 * This is the only way sync gets enabled — signing in alone does not enable it.
 */
export function enableSync(): void {
  collaborationStore.setEnabled(true);
  localStorage.setItem('diaryx_sync_enabled', 'true');
}

/**
 * Check if sync has been explicitly enabled by the user.
 */
export function isSyncEnabled(): boolean {
  return typeof localStorage !== 'undefined' && localStorage.getItem('diaryx_sync_enabled') === 'true';
}

/**
 * Log out and clear auth state.
 */
export async function logout(): Promise<void> {
  const token = getToken();

  // Clear local state first
  state.isAuthenticated = false;
  state.user = null;
  state.workspaces = [];
  state.devices = [];
  state.workspaceLimit = 1;
  state.tier = "free";
  state.publishedSiteLimit = 1;
  state.attachmentLimitBytes = 200 * 1024 * 1024;
  state.activeWorkspaceId = null;
  state.error = null;
  state.storageUsage = null;

  localStorage.removeItem(STORAGE_KEYS.TOKEN);
  localStorage.removeItem(STORAGE_KEYS.USER);
  localStorage.removeItem('diaryx_sync_enabled');

  // Clear collaboration settings
  setAuthToken(undefined);
  setCollaborationWorkspaceId(null);
  collaborationStore.setEnabled(false);

  // Try to logout on server (don't wait for it)
  if (authService && token) {
    authService.logout(token).catch(() => {
      // Ignore logout errors
    });
  }
}

/**
 * Rename a device.
 */
export async function renameDevice(deviceId: string, newName: string): Promise<void> {
  const token = getToken();
  if (!authService || !token) return;

  await authService.renameDevice(token, deviceId, newName);
  await refreshUserInfo();
}

/**
 * Delete a device.
 */
export async function deleteDevice(deviceId: string): Promise<void> {
  const token = getToken();
  if (!authService || !token) return;

  await authService.deleteDevice(token, deviceId);
  await refreshUserInfo();
}

/**
 * Delete account and all server data.
 * This deletes data from the server but preserves local workspace data.
 */
export async function deleteAccount(): Promise<void> {
  const token = getToken();
  if (!authService || !token) return;

  // Delete server data
  await authService.deleteAccount(token);

  // Clear local auth state (but NOT local workspace data)
  state.isAuthenticated = false;
  state.user = null;
  state.workspaces = [];
  state.devices = [];
  state.workspaceLimit = 1;
  state.tier = "free";
  state.publishedSiteLimit = 1;
  state.attachmentLimitBytes = 200 * 1024 * 1024;
  state.activeWorkspaceId = null;
  state.error = null;
  state.storageUsage = null;

  localStorage.removeItem(STORAGE_KEYS.TOKEN);
  localStorage.removeItem(STORAGE_KEYS.USER);
  localStorage.removeItem('diaryx_sync_enabled');

  // Clear collaboration settings
  setAuthToken(undefined);
  setCollaborationWorkspaceId(null);
  collaborationStore.setEnabled(false);
}

// ============================================================================
// Helpers
// ============================================================================

function getDeviceName(): string {
  if (typeof navigator === "undefined") return "Unknown";

  const ua = navigator.userAgent;

  // Check for common browsers/platforms
  if (ua.includes("Chrome")) {
    if (ua.includes("Android")) return "Chrome (Android)";
    if (ua.includes("iPhone") || ua.includes("iPad")) return "Chrome (iOS)";
    if (ua.includes("Windows")) return "Chrome (Windows)";
    if (ua.includes("Mac")) return "Chrome (Mac)";
    if (ua.includes("Linux")) return "Chrome (Linux)";
    return "Chrome";
  }
  if (ua.includes("Firefox")) {
    if (ua.includes("Android")) return "Firefox (Android)";
    if (ua.includes("Windows")) return "Firefox (Windows)";
    if (ua.includes("Mac")) return "Firefox (Mac)";
    if (ua.includes("Linux")) return "Firefox (Linux)";
    return "Firefox";
  }
  if (ua.includes("Safari") && !ua.includes("Chrome")) {
    if (ua.includes("iPhone")) return "Safari (iPhone)";
    if (ua.includes("iPad")) return "Safari (iPad)";
    if (ua.includes("Mac")) return "Safari (Mac)";
    return "Safari";
  }
  if (ua.includes("Tauri")) {
    if (ua.includes("Windows")) return "Diaryx (Windows)";
    if (ua.includes("Mac")) return "Diaryx (Mac)";
    if (ua.includes("Linux")) return "Diaryx (Linux)";
    return "Diaryx Desktop";
  }

  return "Web Browser";
}

// ============================================================================
// Workspace CRUD (server-side)
// ============================================================================

/**
 * Create a new workspace on the server.
 * Refreshes the workspace list on success.
 */
export async function createServerWorkspace(name: string): Promise<Workspace> {
  const token = getToken();
  if (!authService || !token) throw new Error("Not authenticated");
  const ws = await authService.createWorkspace(token, name);
  await refreshUserInfo();
  return ws;
}

/**
 * Rename a workspace on the server.
 * Refreshes the workspace list on success.
 */
export async function renameServerWorkspace(workspaceId: string, newName: string): Promise<void> {
  const token = getToken();
  if (!authService || !token) throw new Error("Not authenticated");
  await authService.renameWorkspace(token, workspaceId, newName);
  await refreshUserInfo();
}

/**
 * Delete a workspace on the server.
 * Refreshes the workspace list on success.
 */
export async function deleteServerWorkspace(workspaceId: string): Promise<void> {
  const token = getToken();
  if (!authService || !token) throw new Error("Not authenticated");
  await authService.deleteWorkspace(token, workspaceId);
  await refreshUserInfo();
}

// ============================================================================
// Stripe Billing
// ============================================================================

/**
 * Create a Stripe Checkout Session for upgrading to Plus.
 * Returns the checkout URL for redirect.
 */
export async function createCheckoutSession(): Promise<string> {
  const token = getToken();
  if (!authService || !token) throw new Error("Not authenticated");
  const { url } = await authService.createCheckoutSession(token);
  return url;
}

/**
 * Create a Stripe Customer Portal session for managing billing.
 * Returns the portal URL for redirect.
 */
export async function createPortalSession(): Promise<string> {
  const token = getToken();
  if (!authService || !token) throw new Error("Not authenticated");
  const { url } = await authService.createPortalSession(token);
  return url;
}

// ============================================================================
// Data Queries
// ============================================================================

/**
 * Check if user has synced data on the server.
 * Returns null if not authenticated or server URL not configured.
 */
export async function checkUserHasData(): Promise<UserHasDataResponse | null> {
  const token = getToken();
  const url = state.serverUrl;
  if (!token || !url || !authService) return null;

  try {
    return await authService.checkUserHasData(token);
  } catch (err) {
    console.error("[AuthStore] Failed to check user data:", err);
    return null;
  }
}

/**
 * Download a workspace snapshot zip from the server.
 */
export async function downloadWorkspaceSnapshot(
  workspaceId: string,
  includeAttachments = true,
): Promise<Blob | null> {
  const token = getToken();
  const url = state.serverUrl;
  if (!token || !url || !authService) return null;

  try {
    return await authService.downloadWorkspaceSnapshot(
      token,
      workspaceId,
      includeAttachments,
    );
  } catch (err) {
    console.error("[AuthStore] Failed to download snapshot:", err);
    return null;
  }
}

/**
 * Upload a workspace snapshot zip to the server.
 */
export async function uploadWorkspaceSnapshot(
  workspaceId: string,
  snapshot: Blob,
  mode: "replace" | "merge" = "replace",
  includeAttachments = true,
): Promise<{ files_imported: number } | null> {
  const token = getToken();
  const url = state.serverUrl;
  if (!token || !url || !authService) return null;

  try {
    const result = await authService.uploadWorkspaceSnapshot(
      token,
      workspaceId,
      snapshot,
      mode,
      includeAttachments,
    );
    await refreshUserStorageUsage();
    return result;
  } catch (err) {
    console.error("[AuthStore] Failed to upload snapshot:", err);
    return null;
  }
}

/**
 * Initialize resumable attachment upload.
 */
export async function initAttachmentUpload(
  workspaceId: string,
  request: InitAttachmentUploadRequest,
): Promise<InitAttachmentUploadResponse> {
  const token = getToken();
  const url = state.serverUrl;
  if (!token || !url || !authService) {
    throw new Error("Not authenticated");
  }
  return authService.initAttachmentUpload(token, workspaceId, request);
}

/**
 * Upload one attachment part.
 */
export async function uploadAttachmentPart(
  workspaceId: string,
  uploadId: string,
  partNo: number,
  bytes: ArrayBuffer,
): Promise<{ ok: boolean; part_no: number }> {
  const token = getToken();
  const url = state.serverUrl;
  if (!token || !url || !authService) {
    throw new Error("Not authenticated");
  }
  return authService.uploadAttachmentPart(token, workspaceId, uploadId, partNo, bytes);
}

/**
 * Complete resumable attachment upload.
 */
export async function completeAttachmentUpload(
  workspaceId: string,
  uploadId: string,
  request: CompleteAttachmentUploadRequest,
): Promise<CompleteAttachmentUploadResponse> {
  const token = getToken();
  const url = state.serverUrl;
  if (!token || !url || !authService) {
    throw new Error("Not authenticated");
  }
  const result = await authService.completeAttachmentUpload(token, workspaceId, uploadId, request);
  await refreshUserStorageUsage();
  return result;
}

/**
 * Download workspace attachment by hash.
 */
export async function downloadAttachment(
  workspaceId: string,
  hash: string,
  range?: { start: number; end?: number },
): Promise<DownloadAttachmentResponse> {
  const token = getToken();
  const url = state.serverUrl;
  if (!token || !url || !authService) {
    throw new Error("Not authenticated");
  }
  return authService.downloadAttachment(token, workspaceId, hash, range);
}
