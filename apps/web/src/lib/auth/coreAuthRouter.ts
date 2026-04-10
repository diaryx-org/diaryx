/**
 * Core auth service router — picks the right `CoreAuthService` impl per
 * runtime (wasm in the browser, Tauri IPC on the Tauri host).
 *
 * Lives in its own module (separate from `index.ts`) so that
 * `authStore.svelte.ts` can import it without creating a circular
 * dependency through `index.ts`.
 */

import { isTauri } from "$lib/backend/interface";
import type { CoreAuthService } from "./coreAuthTypes";
import { createWasmAuthService, resetWasmAuthClient } from "./wasmAuthService";
import {
  createTauriAuthService,
  setTauriAuthServerUrl,
} from "./tauriAuthService";

// The wasm impl needs a live view of the user-configured server URL so it
// can reconstruct the wasm `AuthClient` when the user switches servers.
// Tauri owns its own URL internally and doesn't need this closure.
function readServerUrl(): string | null {
  if (typeof localStorage === "undefined") return null;
  return localStorage.getItem("diaryx_sync_server_url");
}

const wasmService = createWasmAuthService(readServerUrl);
const tauriService = createTauriAuthService();

/**
 * The active `CoreAuthService` for the current runtime.
 *
 * - On the browser, this is a wasm-backed service that runs
 *   `diaryx_core::auth::AuthService` inside WebAssembly and talks to the
 *   sync server via `proxyFetch` with HttpOnly cookies.
 * - On Tauri, this is a thin IPC wrapper that invokes a native Rust
 *   `AuthService<KeyringAuthenticatedClient>` on the host side so the raw
 *   session token is owned by Rust.
 *
 * Both implement the same `CoreAuthService` interface (the 12 methods in
 * the Rust trait), which is how the auth store stays ignorant of the
 * runtime.
 */
export const coreAuthService: CoreAuthService = isTauri()
  ? tauriService
  : wasmService;

/**
 * Notify the active core auth service that the user-configured server URL
 * has changed. On wasm this invalidates the cached `AuthClient`; on Tauri
 * it issues an `auth_set_server_url` IPC call so the host rebuilds its
 * `KeyringAuthenticatedClient`.
 */
export async function setCoreAuthServerUrl(serverUrl: string): Promise<void> {
  if (isTauri()) {
    await setTauriAuthServerUrl(serverUrl);
  } else {
    resetWasmAuthClient();
  }
}
