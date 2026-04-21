/**
 * Core namespace service router — picks the right `CoreNamespaceService`
 * impl per runtime (wasm in the browser, Tauri IPC on the Tauri host).
 *
 * Mirrors `lib/auth/coreAuthRouter.ts`.
 */

import { isTauri } from "$lib/backend/interface";
import type { CoreNamespaceService } from "./coreNamespaceTypes";
import {
  createWasmNamespaceService,
  resetWasmNamespaceClient,
} from "./wasmNamespaceService";
import { createTauriNamespaceService } from "./tauriNamespaceService";

// Same signal the auth router uses so both services track the same
// user-configured server URL without additional state.
function readServerUrl(): string | null {
  if (typeof localStorage === "undefined") return null;
  return localStorage.getItem("diaryx_sync_server_url");
}

const wasmService = createWasmNamespaceService(readServerUrl);
const tauriService = createTauriNamespaceService();

/**
 * The active `CoreNamespaceService` for the current runtime.
 *
 * - On the browser, wasm-backed service that runs
 *   `diaryx_core::namespace::*` inside WebAssembly and talks to the sync
 *   server via `proxyFetch` with HttpOnly cookies.
 * - On Tauri, a thin IPC wrapper that runs the namespace functions natively
 *   using the shared keyring-backed `AuthenticatedClient`.
 */
export const coreNamespaceService: CoreNamespaceService = isTauri()
  ? tauriService
  : wasmService;

/**
 * Notify the active namespace service that the user-configured server URL
 * has changed. On wasm this invalidates the cached `NamespaceClient`; on
 * Tauri it's a no-op because `auth_set_server_url` already rebuilds the
 * shared `KeyringAuthenticatedClient`, which both services use.
 */
export function resetCoreNamespaceClient(): void {
  if (!isTauri()) {
    resetWasmNamespaceClient();
  }
}
