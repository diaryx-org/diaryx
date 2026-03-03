/**
 * Secure credential storage using Tauri Stronghold.
 * Only available in Tauri desktop app, not in WASM.
 */

import { Client, Stronghold } from '@tauri-apps/plugin-stronghold';
import { appDataDir } from '@tauri-apps/api/path';

let strongholdInstance: Stronghold | null = null;
let clientInstance: Client | null = null;
let initPromise: Promise<boolean> | null = null;

const VAULT_FILE = 'diaryx.hold';
const CLIENT_NAME = 'diaryx-credentials';

// App-derived password - this provides encryption without user prompt
// The actual security comes from the OS-level file protection
const APP_DERIVED_PASSWORD = 'diaryx-vault-key-v1';

/**
 * Initialize Stronghold with the app-derived password.
 * Called automatically on first credential access.
 */
async function initCredentialStoreInternal(): Promise<boolean> {
  try {
    console.log('[Credentials] Getting app data dir...');
    const dataDir = await appDataDir();
    const vaultPath = `${dataDir}/${VAULT_FILE}`;
    console.log('[Credentials] Loading Stronghold from:', vaultPath);

    strongholdInstance = await Stronghold.load(vaultPath, APP_DERIVED_PASSWORD);
    console.log('[Credentials] Stronghold loaded, getting client...');

    try {
      clientInstance = await strongholdInstance.loadClient(CLIENT_NAME);
      console.log('[Credentials] Client loaded');
    } catch {
      console.log('[Credentials] Creating new client...');
      clientInstance = await strongholdInstance.createClient(CLIENT_NAME);
      console.log('[Credentials] Client created');
    }

    return true;
  } catch (e) {
    console.error('[Credentials] Failed to init credential store:', e);
    return false;
  }
}

/**
 * Ensure credential store is initialized (auto-init on first use).
 */
async function ensureInitialized(): Promise<void> {
  if (strongholdInstance && clientInstance) return;

  if (!initPromise) {
    initPromise = initCredentialStoreInternal();
  }

  const success = await initPromise;
  if (!success) {
    throw new Error('Failed to initialize credential store');
  }
}

/**
 * Initialize Stronghold with a custom password (legacy API).
 * @deprecated Use the auto-init functions instead
 */
export async function initCredentialStore(password: string): Promise<boolean> {
  try {
    const dataDir = await appDataDir();
    const vaultPath = `${dataDir}/${VAULT_FILE}`;

    strongholdInstance = await Stronghold.load(vaultPath, password);

    try {
      clientInstance = await strongholdInstance.loadClient(CLIENT_NAME);
    } catch {
      clientInstance = await strongholdInstance.createClient(CLIENT_NAME);
    }

    return true;
  } catch (e) {
    console.error('Failed to init credential store:', e);
    return false;
  }
}

/**
 * Check if credential store is initialized.
 */
export function isCredentialStoreReady(): boolean {
  return strongholdInstance !== null && clientInstance !== null;
}

/**
 * Store a credential securely.
 */
export async function storeCredential(key: string, value: string): Promise<void> {
  await ensureInitialized();
  if (!clientInstance || !strongholdInstance) {
    throw new Error('Credential store not initialized.');
  }

  const store = clientInstance.getStore();
  const data = Array.from(new TextEncoder().encode(value));
  await store.insert(key, data);
  await strongholdInstance.save();
}

/**
 * Retrieve a credential.
 */
export async function getCredential(key: string): Promise<string | null> {
  await ensureInitialized();
  if (!clientInstance) {
    throw new Error('Credential store not initialized.');
  }

  const store = clientInstance.getStore();
  try {
    const data = await store.get(key);
    if (!data) return null;
    return new TextDecoder().decode(new Uint8Array(data));
  } catch {
    return null;
  }
}

/**
 * Remove a credential.
 */
export async function removeCredential(key: string): Promise<void> {
  await ensureInitialized();
  if (!clientInstance || !strongholdInstance) {
    throw new Error('Credential store not initialized.');
  }

  const store = clientInstance.getStore();
  await store.remove(key);
  await strongholdInstance.save();
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
