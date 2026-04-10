/**
 * tauriAuthService — Typed wrappers around the Tauri `auth_*` IPC commands.
 *
 * These commands live in `apps/tauri/src-tauri/src/auth_commands.rs` and run
 * a native Rust `AuthService<KeyringAuthenticatedClient>` on the host side.
 * On Tauri, the session token lives in the OS keyring and is never handed
 * back to JavaScript: `verifyMagicLink` / `verifyCode` both return a
 * redacted `VerifyResponse` with `token: ""`.
 *
 * All errors thrown by these commands follow the Tauri convention of
 * `SerializableAuthError`, which we repackage into `AuthError` so callers
 * can treat errors identically across the three `CoreAuthService`
 * implementations.
 */

import { invoke } from "@tauri-apps/api/core";
import type {
  CoreAuthService,
  CoreAuthMetadata,
  Device,
  MagicLinkResponse,
  MeResponse,
  VerifyResponse,
  Workspace,
} from "./coreAuthTypes";
import { AuthError } from "./coreAuthTypes";

/**
 * Convert a thrown IPC error — which Tauri serializes as a plain object or
 * a bare string — into an `AuthError` that keeps `statusCode` + `devices`.
 */
function wrapError(err: unknown): AuthError {
  if (err instanceof AuthError) return err;
  if (err && typeof err === "object") {
    const o = err as {
      message?: string;
      statusCode?: number;
      status_code?: number;
      devices?: Device[];
    };
    const status = o.statusCode ?? o.status_code ?? 0;
    return new AuthError(
      o.message ?? "Auth request failed",
      status,
      undefined,
      o.devices,
    );
  }
  if (typeof err === "string") {
    return new AuthError(err, 0);
  }
  return new AuthError("Auth request failed", 0);
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (err) {
    throw wrapError(err);
  }
}

/**
 * Create a `CoreAuthService` backed by Tauri IPC commands.
 *
 * Unlike `createWasmAuthService`, this implementation does not need a
 * `getServerUrl` closure: the Tauri host owns the server URL internally
 * (persisted to `<app_data>/auth.json`). Use the `setServerUrl` helper
 * below to update it at runtime.
 */
export function createTauriAuthService(): CoreAuthService {
  return {
    async isAuthenticated() {
      try {
        return await invoke<boolean>("auth_is_authenticated");
      } catch {
        return false;
      }
    },

    async getMetadata() {
      try {
        const result = await invoke<CoreAuthMetadata | null>("auth_get_metadata");
        return result ?? null;
      } catch {
        return null;
      }
    },

    requestMagicLink(email) {
      return call<MagicLinkResponse>("auth_request_magic_link", { email });
    },

    verifyMagicLink(token, deviceName, replaceDeviceId) {
      return call<VerifyResponse>("auth_verify_magic_link", {
        token,
        deviceName,
        replaceDeviceId,
      });
    },

    verifyCode(code, email, deviceName, replaceDeviceId) {
      return call<VerifyResponse>("auth_verify_code", {
        code,
        email,
        deviceName,
        replaceDeviceId,
      });
    },

    getMe() {
      return call<MeResponse>("auth_get_me");
    },

    refreshToken() {
      return call<MeResponse>("auth_refresh_token");
    },

    async logout() {
      await call<void>("auth_logout");
    },

    getDevices() {
      return call<Device[]>("auth_get_devices");
    },

    async renameDevice(deviceId, newName) {
      await call<void>("auth_rename_device", { deviceId, newName });
    },

    async deleteDevice(deviceId) {
      await call<void>("auth_delete_device", { deviceId });
    },

    async deleteAccount() {
      await call<void>("auth_delete_account");
    },

    createWorkspace(name) {
      return call<Workspace>("auth_create_workspace", { name });
    },

    async renameWorkspace(workspaceId, newName) {
      await call<void>("auth_rename_workspace", { workspaceId, newName });
    },

    async deleteWorkspace(workspaceId) {
      await call<void>("auth_delete_workspace", { workspaceId });
    },
  };
}

/**
 * Update the server URL that the Tauri host uses for subsequent auth
 * requests. Called by `authStore.setServerUrl()` on Tauri.
 */
export async function setTauriAuthServerUrl(serverUrl: string): Promise<void> {
  await call<void>("auth_set_server_url", { serverUrl });
}

/**
 * Read the server URL currently configured on the Tauri host.
 */
export async function getTauriAuthServerUrl(): Promise<string | null> {
  try {
    return await invoke<string>("auth_server_url");
  } catch {
    return null;
  }
}
