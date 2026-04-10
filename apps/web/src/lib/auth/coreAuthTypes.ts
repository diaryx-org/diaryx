/**
 * Types shared between the three concrete `CoreAuthService` implementations
 * (wasm, tauri IPC, and legacy fetch), all of which hit the same sync server
 * endpoints and must return identical shapes.
 *
 * These mirror the `serde` types in `diaryx_core::auth::types` so that
 * `AuthClient`'s JSON returns deserialize directly into them.
 */

// Re-use the existing User/Workspace/Device/Response types from authService.ts
// to avoid drift — they're the same wire shapes.
export type {
  User,
  Workspace,
  Device,
  DeviceLimitDevice,
  VerifyResponse,
  MeResponse,
  MagicLinkResponse,
} from "./authService";

export { AuthError } from "./authService";

/** Non-secret session metadata (mirrors `diaryx_core::auth::AuthMetadata`). */
export interface CoreAuthMetadata {
  email?: string | null;
  workspace_id?: string | null;
}

/**
 * Narrow surface that the three concrete AuthServices (wasm, tauri, legacy)
 * all implement. Exactly the 12 methods in `diaryx_core::auth::AuthService`.
 */
export interface CoreAuthService {
  isAuthenticated(): Promise<boolean>;
  getMetadata(): Promise<CoreAuthMetadata | null>;

  requestMagicLink(
    email: string,
  ): Promise<import("./authService").MagicLinkResponse>;
  verifyMagicLink(
    token: string,
    deviceName?: string,
    replaceDeviceId?: string,
  ): Promise<import("./authService").VerifyResponse>;
  verifyCode(
    code: string,
    email: string,
    deviceName?: string,
    replaceDeviceId?: string,
  ): Promise<import("./authService").VerifyResponse>;

  getMe(): Promise<import("./authService").MeResponse>;
  refreshToken(): Promise<import("./authService").MeResponse>;
  logout(): Promise<void>;

  getDevices(): Promise<import("./authService").Device[]>;
  renameDevice(deviceId: string, newName: string): Promise<void>;
  deleteDevice(deviceId: string): Promise<void>;

  deleteAccount(): Promise<void>;

  createWorkspace(
    name: string,
  ): Promise<import("./authService").Workspace>;
  renameWorkspace(workspaceId: string, newName: string): Promise<void>;
  deleteWorkspace(workspaceId: string): Promise<void>;
}
