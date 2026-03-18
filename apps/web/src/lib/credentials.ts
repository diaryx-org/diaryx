/**
 * Secure credential storage using the OS keychain via Tauri commands.
 *
 * - macOS / iOS: Keychain Services
 * - Windows: Credential Manager
 * - Linux: Secret Service (GNOME Keyring / KWallet)
 *
 * On Android and browser environments, these functions are not called —
 * the auth layer uses HttpOnly cookies (browser) or falls back to
 * localStorage (Android).
 */

import { invoke } from '@tauri-apps/api/core';

/**
 * Store a credential securely in the OS keychain.
 */
export async function storeCredential(key: string, value: string): Promise<void> {
  await invoke('store_credential', { key, value });
}

/**
 * Retrieve a credential from the OS keychain.
 * Returns null if not found.
 */
export async function getCredential(key: string): Promise<string | null> {
  return await invoke<string | null>('get_credential', { key });
}

/**
 * Remove a credential from the OS keychain.
 */
export async function removeCredential(key: string): Promise<void> {
  await invoke('remove_credential', { key });
}

// Live Sync specific helpers - using localStorage for reliability
const SYNC_SERVER_URL = 'diaryx_sync_server_url';
const SYNC_ENABLED = 'diaryx_sync_enabled';

export interface SyncConfig {
  serverUrl: string;
  enabled: boolean;
}

/**
 * Store sync configuration.
 */
export async function storeSyncConfig(config: SyncConfig): Promise<void> {
  localStorage.setItem(SYNC_SERVER_URL, config.serverUrl);
  localStorage.setItem(SYNC_ENABLED, config.enabled ? 'true' : 'false');
}

/**
 * Get sync configuration.
 */
export async function getSyncConfig(): Promise<SyncConfig | null> {
  try {
    const serverUrl = localStorage.getItem(SYNC_SERVER_URL);
    const enabled = localStorage.getItem(SYNC_ENABLED);

    if (!serverUrl) return null;

    return {
      serverUrl: serverUrl || '',
      enabled: enabled === 'true',
    };
  } catch {
    return null;
  }
}

/**
 * Remove sync configuration.
 */
export async function removeSyncConfig(): Promise<void> {
  localStorage.removeItem(SYNC_SERVER_URL);
  localStorage.removeItem(SYNC_ENABLED);
}
