import { isTauri } from "$lib/backend/interface";

// ============================================================================
// Browser secret storage: CryptoKey + IndexedDB
//
// Secrets are encrypted with a non-extractable AES-GCM key stored in IndexedDB.
// This protects against casual localStorage inspection, enumeration, and
// data-at-rest exposure. The CryptoKey persists across sessions.
//
// On Tauri, the system keychain is used instead (via $lib/credentials).
// ============================================================================

const DB_NAME = "diaryx-plugin-secrets";
const DB_VERSION = 1;
const KEY_STORE = "encryption-keys";
const SECRET_STORE = "secrets";
const ENCRYPTION_KEY_ID = "plugin-secret-key";

/** Open (or create) the IndexedDB database for plugin secrets. */
function openSecretsDb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, DB_VERSION);
    request.onupgradeneeded = () => {
      const db = request.result;
      if (!db.objectStoreNames.contains(KEY_STORE)) {
        db.createObjectStore(KEY_STORE);
      }
      if (!db.objectStoreNames.contains(SECRET_STORE)) {
        db.createObjectStore(SECRET_STORE);
      }
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

/** Get or create the non-extractable AES-GCM encryption key. */
async function getOrCreateEncryptionKey(): Promise<CryptoKey> {
  const db = await openSecretsDb();

  // Try to load existing key
  const existing = await new Promise<CryptoKey | undefined>((resolve, reject) => {
    const tx = db.transaction(KEY_STORE, "readonly");
    const store = tx.objectStore(KEY_STORE);
    const request = store.get(ENCRYPTION_KEY_ID);
    request.onsuccess = () => resolve(request.result as CryptoKey | undefined);
    request.onerror = () => reject(request.error);
  });

  if (existing) {
    db.close();
    return existing;
  }

  // Generate a new non-extractable key
  const key = await crypto.subtle.generateKey(
    { name: "AES-GCM", length: 256 },
    false, // non-extractable: JS cannot read the raw key bytes
    ["encrypt", "decrypt"],
  );

  // Store the CryptoKey object in IndexedDB (IDB can store CryptoKey directly)
  await new Promise<void>((resolve, reject) => {
    const tx = db.transaction(KEY_STORE, "readwrite");
    const store = tx.objectStore(KEY_STORE);
    const request = store.put(key, ENCRYPTION_KEY_ID);
    request.onsuccess = () => resolve();
    request.onerror = () => reject(request.error);
  });

  db.close();
  return key;
}

/** Encrypt a string value with AES-GCM. Returns iv + ciphertext as a single ArrayBuffer. */
async function encryptValue(key: CryptoKey, plaintext: string): Promise<ArrayBuffer> {
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const encoded = new TextEncoder().encode(plaintext);
  const ciphertext = await crypto.subtle.encrypt(
    { name: "AES-GCM", iv },
    key,
    encoded,
  );
  // Concatenate: [12 bytes IV][ciphertext]
  const result = new Uint8Array(iv.length + ciphertext.byteLength);
  result.set(iv, 0);
  result.set(new Uint8Array(ciphertext), iv.length);
  return result.buffer;
}

/** Decrypt an iv+ciphertext buffer back to a string. */
async function decryptValue(key: CryptoKey, data: ArrayBuffer): Promise<string> {
  const bytes = new Uint8Array(data);
  const iv = bytes.slice(0, 12);
  const ciphertext = bytes.slice(12);
  const decrypted = await crypto.subtle.decrypt(
    { name: "AES-GCM", iv },
    key,
    ciphertext,
  );
  return new TextDecoder().decode(decrypted);
}

function secretDbKey(pluginId: string, key: string): string {
  return `${pluginId}:${key}`;
}

// Legacy localStorage key format (for migration)
const LEGACY_PREFIX = "diaryx-plugin-secret";
function legacyKey(pluginId: string, key: string): string {
  return `${LEGACY_PREFIX}:${pluginId}:${key}`;
}

// ============================================================================
// IndexedDB secret get/set/delete (browser path)
// ============================================================================

async function idbGetSecret(pluginId: string, key: string): Promise<string | null> {
  const db = await openSecretsDb();
  const dbKey = secretDbKey(pluginId, key);
  const stored = await new Promise<ArrayBuffer | undefined>((resolve, reject) => {
    const tx = db.transaction(SECRET_STORE, "readonly");
    const store = tx.objectStore(SECRET_STORE);
    const request = store.get(dbKey);
    request.onsuccess = () => resolve(request.result as ArrayBuffer | undefined);
    request.onerror = () => reject(request.error);
  });
  db.close();

  if (!stored) {
    // Migration: check localStorage for legacy secret
    try {
      const legacy = localStorage.getItem(legacyKey(pluginId, key));
      if (legacy !== null) {
        // Migrate to encrypted IndexedDB storage, then remove from localStorage
        await idbSetSecret(pluginId, key, legacy);
        localStorage.removeItem(legacyKey(pluginId, key));
        return legacy;
      }
    } catch {
      // localStorage may be unavailable
    }
    return null;
  }

  const encryptionKey = await getOrCreateEncryptionKey();
  return decryptValue(encryptionKey, stored);
}

async function idbSetSecret(pluginId: string, key: string, value: string): Promise<void> {
  const encryptionKey = await getOrCreateEncryptionKey();
  const encrypted = await encryptValue(encryptionKey, value);

  const db = await openSecretsDb();
  const dbKey = secretDbKey(pluginId, key);
  await new Promise<void>((resolve, reject) => {
    const tx = db.transaction(SECRET_STORE, "readwrite");
    const store = tx.objectStore(SECRET_STORE);
    const request = store.put(encrypted, dbKey);
    request.onsuccess = () => resolve();
    request.onerror = () => reject(request.error);
  });
  db.close();

  // Clean up any legacy localStorage entry
  try {
    localStorage.removeItem(legacyKey(pluginId, key));
  } catch {
    // localStorage may be unavailable
  }
}

async function idbDeleteSecret(pluginId: string, key: string): Promise<void> {
  const db = await openSecretsDb();
  const dbKey = secretDbKey(pluginId, key);
  await new Promise<void>((resolve, reject) => {
    const tx = db.transaction(SECRET_STORE, "readwrite");
    const store = tx.objectStore(SECRET_STORE);
    const request = store.delete(dbKey);
    request.onsuccess = () => resolve();
    request.onerror = () => reject(request.error);
  });
  db.close();

  // Clean up any legacy localStorage entry
  try {
    localStorage.removeItem(legacyKey(pluginId, key));
  } catch {
    // localStorage may be unavailable
  }
}

// ============================================================================
// Public API
// ============================================================================

export async function getPluginSecret(
  pluginId: string,
  key: string,
): Promise<string | null> {
  if (isTauri()) {
    try {
      const { getCredential } = await import("$lib/credentials");
      return await getCredential(legacyKey(pluginId, key));
    } catch (error) {
      console.warn("[pluginSecretStore] falling back to IndexedDB get", {
        pluginId,
        key,
        error,
      });
    }
  }

  try {
    return await idbGetSecret(pluginId, key);
  } catch (error) {
    console.warn("[pluginSecretStore] IndexedDB get failed", { pluginId, key, error });
    return null;
  }
}

export async function setPluginSecret(
  pluginId: string,
  key: string,
  value: string,
): Promise<void> {
  if (isTauri()) {
    try {
      const { storeCredential } = await import("$lib/credentials");
      await storeCredential(legacyKey(pluginId, key), value);
      return;
    } catch (error) {
      console.warn("[pluginSecretStore] falling back to IndexedDB set", {
        pluginId,
        key,
        error,
      });
    }
  }

  try {
    await idbSetSecret(pluginId, key, value);
  } catch (error) {
    console.warn("[pluginSecretStore] IndexedDB set failed", { pluginId, key, error });
  }
}

export async function deletePluginSecret(
  pluginId: string,
  key: string,
): Promise<void> {
  if (isTauri()) {
    try {
      const { removeCredential } = await import("$lib/credentials");
      await removeCredential(legacyKey(pluginId, key));
      return;
    } catch (error) {
      console.warn("[pluginSecretStore] falling back to IndexedDB delete", {
        pluginId,
        key,
        error,
      });
    }
  }

  try {
    await idbDeleteSecret(pluginId, key);
  } catch (error) {
    console.warn("[pluginSecretStore] IndexedDB delete failed", { pluginId, key, error });
  }
}
