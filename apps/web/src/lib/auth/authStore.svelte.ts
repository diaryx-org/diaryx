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
  type DeviceLimitDevice,
  type PasskeyListItem,
  AuthError,
  type NamespaceEntry,
  type UserHasDataResponse,
  type UserStorageUsageResponse,
  type InitAttachmentUploadRequest,
  type InitAttachmentUploadResponse,
  type CompleteAttachmentUploadRequest,
  type CompleteAttachmentUploadResponse,
  type DownloadAttachmentResponse,
} from "./authService";
import {
  prepareCreationOptions,
  prepareRequestOptions,
  serializeRegistrationCredential,
  serializeAuthenticationCredential,
} from "./webauthnUtils";
import { collaborationStore } from "@/models/stores/collaborationStore.svelte";
import { getCurrentWorkspaceId as registryGetCurrentWorkspaceId } from "$lib/storage/localWorkspaceRegistry.svelte";
import { isTauri } from "$lib/backend/interface";
import { proxyFetch } from "$lib/backend/proxyFetch";

function setAuthToken(_token: string | undefined): void {
  // Sync plugin reads token through host callbacks/local auth state.
}

function setCollaborationWorkspaceId(_workspaceId: string | null): void {
  // Workspace sync ownership moved to the sync plugin.
}

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
// Device Replacement
// ============================================================================

/** When a sign-in attempt hits the device limit, this holds the pending
 *  verification context so the UI can show a device picker and retry. */
export interface DeviceReplacementContext {
  devices: DeviceLimitDevice[];
  /** Which auth flow hit the limit. */
  kind: "magic_link" | "code" | "passkey";
  /** Stashed args needed to retry the verification. */
  args: Record<string, unknown>;
}

let deviceReplacementCtx = $state<DeviceReplacementContext | null>(null);

export function getDeviceReplacementContext(): DeviceReplacementContext | null {
  return deviceReplacementCtx;
}

export function clearDeviceReplacement(): void {
  deviceReplacementCtx = null;
}

/**
 * Retry the pending sign-in after the user picks a device to replace.
 */
export async function retryWithDeviceReplacement(
  replaceDeviceId: string,
): Promise<void> {
  const ctx = deviceReplacementCtx;
  if (!ctx) throw new Error("No pending device replacement");
  deviceReplacementCtx = null;

  const args = ctx.args;
  if (ctx.kind === "magic_link") {
    await verifyMagicLink(
      args.token as string,
      args.customDeviceName as string | undefined,
      replaceDeviceId,
    );
  } else if (ctx.kind === "code") {
    await verifyCode(
      args.code as string,
      args.email as string,
      args.customDeviceName as string | undefined,
      replaceDeviceId,
    );
  } else if (ctx.kind === "passkey") {
    await authenticateWithPasskey(
      args.email as string | undefined,
      args.customDeviceName as string | undefined,
      replaceDeviceId,
    );
  }
}

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

/**
 * Get the auth token.
 *
 * - Browser: returns `null` — the token lives in an HttpOnly cookie that JS
 *   cannot read. Auth happens automatically via `credentials: 'include'`.
 * - Tauri: reads the token from Stronghold (async). The synchronous signature
 *   returns `null`; callers that need the raw token on Tauri should use
 *   `getTokenAsync()` instead.
 */
export function getToken(): string | null {
  // On browser the HttpOnly cookie is invisible to JS — return null.
  // On Tauri, proxyFetch auto-injects the Bearer header from Stronghold,
  // so most callers don't need the raw token.
  return null;
}

/**
 * Async token accessor for Tauri (reads from Stronghold).
 * On browser, always returns null (cookie-based auth).
 */
export async function getTokenAsync(): Promise<string | null> {
  if (!isTauri()) return null;
  try {
    const { getCredential } = await import("$lib/credentials");
    return await getCredential(STORAGE_KEYS.TOKEN);
  } catch {
    return null;
  }
}

export function getServerUrl(): string | null {
  return state.serverUrl;
}

export function getDefaultWorkspace(): Workspace | null {
  if (!state.workspaces) return null;
  if (state.activeWorkspaceId) {
    const active = state.workspaces.find((w) => w.id === state.activeWorkspaceId);
    if (active) return active;
    // activeWorkspaceId is set but not in server workspace list — this is a
    // local-only workspace. Don't fall back to an unrelated server workspace,
    // as that would cause the wrong workspace's data to sync in.
    return null;
  }
  return state.workspaces[0] ?? null;
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
  if (currentId && state.workspaces) {
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
 * Check whether the sync server is reachable.
 * Uses the lightweight `/api/health` endpoint with a short timeout.
 * Returns true if the server responded with a 2xx status.
 */
export async function checkServerHealth(serverUrl: string): Promise<boolean> {
  try {
    const base = serverUrl.replace(/\/+$/, '');
    const url = base.endsWith('/api') ? `${base}/health` : `${base}/api/health`;
    const resp = await proxyFetch(url, { method: "GET", timeout_ms: 5000 });
    return resp.ok;
  } catch {
    return false;
  }
}

/**
 * Attempt to reconnect to the sync server.
 * Call this when the user explicitly asks to go back online.
 */
export async function reconnectServer(): Promise<boolean> {
  const serverUrl = state.serverUrl;
  if (!serverUrl) return false;

  const healthy = await checkServerHealth(serverUrl);
  if (healthy) {
    collaborationStore.setServerOffline(false);
    // Re-run auth initialization to restore session
    await initAuth();
    return true;
  }
  return false;
}

/**
 * Initialize auth state.
 *
 * - Browser: validates session via cookie (calls `/api/auth/me` with
 *   `credentials: 'include'` — no token needed).
 * - Tauri: loads token from Stronghold, validates with server.
 */
export async function initAuth(): Promise<void> {
  if (typeof localStorage === "undefined") return;

  const serverUrl = localStorage.getItem(STORAGE_KEYS.SERVER_URL);
  const savedUser = localStorage.getItem(STORAGE_KEYS.USER);

  if (serverUrl) {
    state.serverUrl = serverUrl;
    authService = createAuthService(serverUrl);
  }

  // On Tauri, load token from Stronghold
  const token = await getTokenAsync();

  // On browser, we don't need a token — the cookie is sent automatically.
  // We still need to validate the session by calling /auth/me.
  const hasSession = isTauri() ? !!token : !!serverUrl;

  if (hasSession && serverUrl) {
    // Health check: if server is unreachable, enter offline mode
    const healthy = await checkServerHealth(serverUrl);
    if (!healthy) {
      console.warn("[AuthStore] Server unreachable, entering offline mode");
      collaborationStore.setServerOffline(true);
      // Restore cached user so UI still shows account info
      if (savedUser) {
        try {
          state.user = JSON.parse(savedUser);
          state.isAuthenticated = true;
        } catch {
          // Invalid saved user
        }
      }
      state.isLoading = false;
      return;
    }

    // Server is reachable — clear any previous offline state
    collaborationStore.setServerOffline(false);

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
      // Validate session with server (cookie or Stronghold token via proxyFetch)
      const me = await authService!.getMe(token ?? undefined);
      state.user = me.user;
      state.workspaces = me.workspaces;
      state.devices = me.devices;
      state.workspaceLimit = me.workspace_limit;
      state.tier = me.tier;
      state.publishedSiteLimit = me.published_site_limit;
      state.attachmentLimitBytes = me.attachment_limit_bytes;
      state.isAuthenticated = true;

      // Update collaboration settings
      setAuthToken(token ?? undefined);
      const activeWorkspace = getCurrentWorkspace();
      if (activeWorkspace) {
        setCollaborationWorkspaceId(activeWorkspace.id);
      }

      // Only re-enable sync if user previously completed sync setup
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
        // Session expired, clear auth state
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
 * from AddWorkspaceDialog after authentication completes.
 */
export function setServerUrl(url: string | null): void {
  state.serverUrl = url;

  if (url) {
    localStorage.setItem(STORAGE_KEYS.SERVER_URL, url);
    authService = createAuthService(url);
    // Note: We intentionally do NOT call setCollaborationServer() here.
    // Sync should only start after authentication completes and user
    // chooses to sync via AddWorkspaceDialog.
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
): Promise<{ success: boolean; devLink?: string; devCode?: string }> {
  if (!authService) {
    throw new Error("Server URL not configured");
  }

  state.isLoading = true;
  state.error = null;

  try {
    const response = await authService.requestMagicLink(email);
    return { success: true, devLink: response.dev_link, devCode: response.dev_code };
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
 * @param replaceDeviceId - If provided, replace this device to make room when at the device limit.
 */
export async function verifyMagicLink(token: string, customDeviceName?: string, replaceDeviceId?: string): Promise<void> {
  if (!authService) {
    throw new Error("Server URL not configured");
  }

  state.isLoading = true;
  state.error = null;

  try {
    // Use custom name if provided, otherwise auto-detect
    const deviceName = customDeviceName?.trim() || getDeviceName();

    const response = await authService.verifyMagicLink(token, deviceName, replaceDeviceId);

    // Store token: Stronghold on Tauri, cookie on browser (set by server)
    if (isTauri()) {
      const { storeCredential } = await import("$lib/credentials");
      await storeCredential(STORAGE_KEYS.TOKEN, response.token);
    }
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
    if (err instanceof AuthError && err.statusCode === 403 && err.devices) {
      deviceReplacementCtx = {
        devices: err.devices,
        kind: "magic_link",
        args: { token, customDeviceName },
      };
    }
    const message =
      err instanceof Error ? err.message : "Failed to verify magic link";
    state.error = message;
    throw err;
  } finally {
    state.isLoading = false;
  }
}

/**
 * Verify a 6-digit code and log in.
 */
export async function verifyCode(
  code: string,
  email: string,
  customDeviceName?: string,
  replaceDeviceId?: string,
): Promise<void> {
  if (!authService) {
    throw new Error("Server URL not configured");
  }

  state.isLoading = true;
  state.error = null;

  try {
    const deviceName = customDeviceName?.trim() || getDeviceName();
    const response = await authService.verifyCode(code, email, deviceName, replaceDeviceId);

    // Store token: Stronghold on Tauri, cookie on browser (set by server)
    if (isTauri()) {
      const { storeCredential } = await import("$lib/credentials");
      await storeCredential(STORAGE_KEYS.TOKEN, response.token);
    }
    localStorage.setItem(STORAGE_KEYS.USER, JSON.stringify(response.user));

    // Update state
    state.user = response.user;
    state.isAuthenticated = true;

    // Update collaboration settings
    setAuthToken(response.token);

    // Fetch full user info (workspaces, devices)
    await refreshUserInfo();
    await refreshUserStorageUsage();
  } catch (err) {
    if (err instanceof AuthError && err.statusCode === 403 && err.devices) {
      deviceReplacementCtx = {
        devices: err.devices,
        kind: "code",
        args: { code, email, customDeviceName },
      };
    }
    const message =
      err instanceof Error ? err.message : "Failed to verify code";
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
  if (!authService) return;
  const token = await getTokenAsync();
  // On browser, token is null but cookie auth works via proxyFetch

  try {
    const me = await authService.getMe(token ?? undefined);
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
 * List namespaces owned by the authenticated user.
 * Works before any workspace/plugin is loaded — calls the server directly.
 */
export async function listUserNamespaces(): Promise<NamespaceEntry[]> {
  if (!authService || !state.isAuthenticated) return [];
  const token = await getTokenAsync();
  try {
    return await authService.listNamespaces(token ?? undefined);
  } catch (err) {
    console.error("[AuthStore] Failed to list namespaces:", err);
    return [];
  }
}

/**
 * List namespaces that have workspace metadata (kind === "workspace").
 */
export async function listUserWorkspaceNamespaces(): Promise<NamespaceEntry[]> {
  const all = await listUserNamespaces();
  return all.filter((ns) => ns.metadata?.kind === "workspace");
}

/**
 * Refresh attachment storage usage from server.
 */
export async function refreshUserStorageUsage(): Promise<void> {
  const url = state.serverUrl;
  if (!url || !authService || !state.isAuthenticated) {
    state.storageUsage = null;
    return;
  }
  const token = await getTokenAsync();

  try {
    state.storageUsage = await authService.getUserStorageUsage(token ?? undefined);
  } catch (err) {
    console.error("[AuthStore] Failed to refresh storage usage:", err);
  }
}

/**
 * Explicitly enable sync. Called by AddWorkspaceDialog after workspace initialization.
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
  const token = await getTokenAsync();

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

  // Clear Stronghold on Tauri
  if (isTauri()) {
    try {
      const { removeCredential } = await import("$lib/credentials");
      await removeCredential(STORAGE_KEYS.TOKEN);
    } catch {
      // Stronghold not available
    }
  }

  localStorage.removeItem(STORAGE_KEYS.USER);
  localStorage.removeItem('diaryx_sync_enabled');

  // Clear collaboration settings
  setAuthToken(undefined);
  setCollaborationWorkspaceId(null);
  collaborationStore.setEnabled(false);

  // Try to logout on server (clears cookie on browser, invalidates session)
  if (authService) {
    authService.logout(token ?? undefined).catch(() => {
      // Ignore logout errors
    });
  }
}

/**
 * Rename a device.
 */
export async function renameDevice(deviceId: string, newName: string): Promise<void> {
  const token = await getTokenAsync();
  if (!authService) return;

  await authService.renameDevice(token ?? undefined, deviceId, newName);
  await refreshUserInfo();
}

/**
 * Delete a device.
 */
export async function deleteDevice(deviceId: string): Promise<void> {
  const token = await getTokenAsync();
  if (!authService) return;

  await authService.deleteDevice(token ?? undefined, deviceId);
  await refreshUserInfo();
}

/**
 * Delete account and all server data.
 * This deletes data from the server but preserves local workspace data.
 */
export async function deleteAccount(): Promise<void> {
  const token = await getTokenAsync();
  if (!authService) return;

  // Delete server data
  await authService.deleteAccount(token ?? undefined);

  // Clear Stronghold on Tauri
  if (isTauri()) {
    try {
      const { removeCredential } = await import("$lib/credentials");
      await removeCredential(STORAGE_KEYS.TOKEN);
    } catch {
      // Stronghold not available
    }
  }

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
  const token = await getTokenAsync();
  if (!authService) throw new Error("Not authenticated");
  const ws = await authService.createWorkspace(token ?? undefined, name);
  await refreshUserInfo();
  return ws;
}

/**
 * Rename a workspace on the server.
 * Refreshes the workspace list on success.
 */
export async function renameServerWorkspace(workspaceId: string, newName: string): Promise<void> {
  const token = await getTokenAsync();
  if (!authService) throw new Error("Not authenticated");
  await authService.renameWorkspace(token ?? undefined, workspaceId, newName);
  await refreshUserInfo();
}

/**
 * Delete a workspace on the server.
 * Refreshes the workspace list on success.
 */
export async function deleteServerWorkspace(workspaceId: string): Promise<void> {
  const token = await getTokenAsync();
  if (!authService) throw new Error("Not authenticated");
  await authService.deleteWorkspace(token ?? undefined, workspaceId);
  await refreshUserInfo();
  await refreshUserStorageUsage();
}

// ============================================================================
// Stripe Billing
// ============================================================================

/**
 * Create a Stripe Checkout Session for upgrading to Plus.
 * Returns the checkout URL for redirect.
 */
export async function createCheckoutSession(): Promise<string> {
  const token = await getTokenAsync();
  if (!authService) throw new Error("Not authenticated");
  const { url } = await authService.createCheckoutSession(token ?? undefined);
  return url;
}

/**
 * Create a Stripe Customer Portal session for managing billing.
 * Returns the portal URL for redirect.
 */
export async function createPortalSession(): Promise<string> {
  const token = await getTokenAsync();
  if (!authService) throw new Error("Not authenticated");
  const { url } = await authService.createPortalSession(token ?? undefined);
  return url;
}

// ============================================================================
// Apple IAP
// ============================================================================

/**
 * Verify an Apple StoreKit 2 signed transaction with the server.
 * Upgrades to Plus on success and refreshes user info.
 */
export async function verifyAppleTransaction(
  signedTransaction: string,
  productId: string,
): Promise<void> {
  const token = await getTokenAsync();
  if (!authService) throw new Error("Not authenticated");
  await authService.verifyAppleTransaction(token ?? undefined, signedTransaction, productId);
  await refreshUserInfo();
}

/**
 * Restore Apple IAP purchases with the server.
 * Refreshes user info after restore.
 */
export async function restoreApplePurchases(
  signedTransactions: string[],
): Promise<{ restored_count: number; tier: string }> {
  const token = await getTokenAsync();
  if (!authService) throw new Error("Not authenticated");
  const result = await authService.restoreApplePurchases(
    token ?? undefined,
    signedTransactions,
  );
  await refreshUserInfo();
  return result;
}

// ============================================================================
// Passkeys (WebAuthn)
// ============================================================================

/**
 * Register a new passkey for the current user.
 * Triggers the browser's platform authenticator (Touch ID, Face ID, etc).
 */
export async function registerPasskey(name: string): Promise<string> {
  const token = await getTokenAsync();
  if (!authService) throw new Error("Not authenticated");

  // 1. Start registration on server
  const { challenge_id, options } =
    await authService.startPasskeyRegistration(token ?? undefined);

  // 2. Create credential with browser
  const creationOptions = prepareCreationOptions(options);
  const credential = (await navigator.credentials.create(
    creationOptions,
  )) as PublicKeyCredential | null;

  if (!credential) throw new Error("Passkey registration was cancelled");

  // 3. Send credential to server
  const serialized = serializeRegistrationCredential(credential);
  const { id } = await authService.finishPasskeyRegistration(
    token ?? undefined,
    challenge_id,
    name,
    serialized,
  );

  return id;
}

/**
 * Authenticate with a passkey (no session required — used at sign-in).
 * If email is provided, scopes to that user's passkeys.
 * If omitted, uses discoverable credentials (browser picks).
 */
export async function authenticateWithPasskey(
  email?: string,
  customDeviceName?: string,
  replaceDeviceId?: string,
): Promise<void> {
  if (!authService) throw new Error("Server URL not configured");

  state.isLoading = true;
  state.error = null;

  try {
    // 1. Start authentication on server
    const { challenge_id, options } =
      await authService.startPasskeyAuthentication(email);

    // 2. Get assertion from browser
    const requestOptions = prepareRequestOptions(options);
    const credential = (await navigator.credentials.get(
      requestOptions,
    )) as PublicKeyCredential | null;

    if (!credential) throw new Error("Passkey authentication was cancelled");

    // 3. Complete authentication on server
    const deviceName = customDeviceName?.trim() || getDeviceName();
    const serialized = serializeAuthenticationCredential(credential);
    const response = await authService.finishPasskeyAuthentication(
      challenge_id,
      serialized,
      deviceName,
      replaceDeviceId,
    );

    // 4. Same post-login flow as verifyMagicLink
    if (isTauri()) {
      const { storeCredential } = await import("$lib/credentials");
      await storeCredential(STORAGE_KEYS.TOKEN, response.token);
    }
    localStorage.setItem(STORAGE_KEYS.USER, JSON.stringify(response.user));

    state.user = response.user;
    state.isAuthenticated = true;

    setAuthToken(response.token);

    await refreshUserInfo();
    await refreshUserStorageUsage();
  } catch (err) {
    if (err instanceof AuthError && err.statusCode === 403 && err.devices) {
      deviceReplacementCtx = {
        devices: err.devices,
        kind: "passkey",
        args: { email, customDeviceName },
      };
    }
    const message =
      err instanceof Error ? err.message : "Failed to authenticate with passkey";
    state.error = message;
    throw err;
  } finally {
    state.isLoading = false;
  }
}

/**
 * List the current user's passkeys.
 */
export async function listPasskeys(): Promise<PasskeyListItem[]> {
  const token = await getTokenAsync();
  if (!authService) return [];
  try {
    return await authService.listPasskeys(token ?? undefined);
  } catch {
    return [];
  }
}

/**
 * Delete a passkey.
 */
export async function deletePasskey(id: string): Promise<void> {
  const token = await getTokenAsync();
  if (!authService) throw new Error("Not authenticated");
  await authService.deletePasskey(token ?? undefined, id);
}

// ============================================================================
// Data Queries
// ============================================================================

/**
 * Check if user has synced data on the server.
 * Returns null if not authenticated or server URL not configured.
 */
export async function checkUserHasData(): Promise<UserHasDataResponse | null> {
  const url = state.serverUrl;
  if (!url || !authService) return null;
  const token = await getTokenAsync();

  try {
    return await authService.checkUserHasData(token ?? undefined);
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
  commitId?: string,
): Promise<Blob | null> {
  const url = state.serverUrl;
  if (!url || !authService) return null;
  const token = await getTokenAsync();

  try {
    return await authService.downloadWorkspaceSnapshot(
      token ?? undefined,
      workspaceId,
      includeAttachments,
      commitId,
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
  onUploadProgress?: (uploadedBytes: number, totalBytes: number) => void,
): Promise<{ files_imported: number } | null> {
  const url = state.serverUrl;
  if (!url || !authService) {
    throw new Error("Not authenticated");
  }
  const token = await getTokenAsync();

  try {
    const result = await authService.uploadWorkspaceSnapshot(
      token ?? undefined,
      workspaceId,
      snapshot,
      mode,
      includeAttachments,
      onUploadProgress,
    );
    await refreshUserStorageUsage();
    return result;
  } catch (err) {
    console.error("[AuthStore] Failed to upload snapshot:", err);
    if (err instanceof Error) throw err;
    throw new Error("Failed to upload snapshot");
  }
}

/**
 * Initialize resumable attachment upload.
 */
export async function initAttachmentUpload(
  workspaceId: string,
  request: InitAttachmentUploadRequest,
): Promise<InitAttachmentUploadResponse> {
  const url = state.serverUrl;
  if (!url || !authService) {
    throw new Error("Not authenticated");
  }
  const token = await getTokenAsync();
  return authService.initAttachmentUpload(token ?? undefined, workspaceId, request);
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
  const url = state.serverUrl;
  if (!url || !authService) {
    throw new Error("Not authenticated");
  }
  const token = await getTokenAsync();
  return authService.uploadAttachmentPart(token ?? undefined, workspaceId, uploadId, partNo, bytes);
}

/**
 * Complete resumable attachment upload.
 */
export async function completeAttachmentUpload(
  workspaceId: string,
  uploadId: string,
  request: CompleteAttachmentUploadRequest,
): Promise<CompleteAttachmentUploadResponse> {
  const url = state.serverUrl;
  if (!url || !authService) {
    throw new Error("Not authenticated");
  }
  const token = await getTokenAsync();
  const result = await authService.completeAttachmentUpload(token ?? undefined, workspaceId, uploadId, request);
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
  const url = state.serverUrl;
  if (!url || !authService) {
    throw new Error("Not authenticated");
  }
  const token = await getTokenAsync();
  return authService.downloadAttachment(token ?? undefined, workspaceId, hash, range);
}
