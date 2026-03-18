//! Secure credential storage.
//!
//! ## Desktop / iOS (`cfg(not(target_os = "android"))`)
//!
//! Uses `keyring-rs` backed by the OS keychain:
//! - macOS / iOS: Keychain Services
//! - Windows: Credential Manager
//! - Linux: Secret Service (GNOME Keyring / KWallet)
//!
//! ## Android (`cfg(target_os = "android")`)
//!
//! Falls back to an XOR-obfuscated JSON file in the app's private data
//! directory. Android sandboxes each app's data dir, so other apps cannot
//! read it. The obfuscation prevents credentials from being stored as
//! plain text on disk.

use std::path::PathBuf;

/// Tauri-managed state that holds the app data directory path.
/// Needed on Android for the file-based credential fallback; unused on
/// desktop/iOS where keyring-rs talks directly to the OS keychain.
#[allow(dead_code)]
pub struct CredentialStoreDir(pub PathBuf);

// =========================================================================
// Desktop / iOS — OS keychain via keyring-rs
// =========================================================================

#[cfg(not(target_os = "android"))]
mod platform {
    const SERVICE_NAME: &str = "org.diaryx.app";

    pub fn store(key: &str, value: &str) -> Result<(), String> {
        let entry = keyring::Entry::new(SERVICE_NAME, key).map_err(|e| e.to_string())?;
        entry.set_password(value).map_err(|e| e.to_string())
    }

    pub fn get(key: &str) -> Result<Option<String>, String> {
        let entry = keyring::Entry::new(SERVICE_NAME, key).map_err(|e| e.to_string())?;
        match entry.get_password() {
            Ok(val) => Ok(Some(val)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn remove(key: &str) -> Result<(), String> {
        let entry = keyring::Entry::new(SERVICE_NAME, key).map_err(|e| e.to_string())?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

// =========================================================================
// Android — XOR-obfuscated JSON file in app-private data dir
// =========================================================================

#[cfg(target_os = "android")]
mod platform {
    use std::collections::HashMap;
    use std::fs;
    use std::path::Path;

    /// Static obfuscation key. This is not cryptographic security — it just
    /// prevents credentials from sitting as plain text on disk. The real
    /// security boundary is Android's per-app data directory sandboxing.
    const OBF_KEY: &[u8] = b"diaryx-android-credential-obf-v1";
    const STORE_FILE: &str = "credentials.bin";

    fn xor_transform(data: &[u8]) -> Vec<u8> {
        data.iter()
            .enumerate()
            .map(|(i, b)| b ^ OBF_KEY[i % OBF_KEY.len()])
            .collect()
    }

    fn store_path(data_dir: &Path) -> std::path::PathBuf {
        data_dir.join(STORE_FILE)
    }

    fn read_store(data_dir: &Path) -> HashMap<String, String> {
        let path = store_path(data_dir);
        let Ok(obfuscated) = fs::read(&path) else {
            return HashMap::new();
        };
        let json_bytes = xor_transform(&obfuscated);
        serde_json::from_slice(&json_bytes).unwrap_or_default()
    }

    fn write_store(data_dir: &Path, map: &HashMap<String, String>) -> Result<(), String> {
        let path = store_path(data_dir);
        let json = serde_json::to_vec(map).map_err(|e| e.to_string())?;
        let obfuscated = xor_transform(&json);
        fs::write(&path, obfuscated).map_err(|e| e.to_string())
    }

    pub fn store(data_dir: &Path, key: &str, value: &str) -> Result<(), String> {
        let mut map = read_store(data_dir);
        map.insert(key.to_string(), value.to_string());
        write_store(data_dir, &map)
    }

    pub fn get(data_dir: &Path, key: &str) -> Result<Option<String>, String> {
        let map = read_store(data_dir);
        Ok(map.get(key).cloned())
    }

    pub fn remove(data_dir: &Path, key: &str) -> Result<(), String> {
        let mut map = read_store(data_dir);
        if map.remove(key).is_some() {
            write_store(data_dir, &map)?;
        }
        Ok(())
    }
}

// =========================================================================
// Tauri commands — delegate to the platform module
// =========================================================================

#[cfg(not(target_os = "android"))]
#[tauri::command]
pub fn store_credential(key: String, value: String) -> Result<(), String> {
    platform::store(&key, &value)
}

#[cfg(not(target_os = "android"))]
#[tauri::command]
pub fn get_credential(key: String) -> Result<Option<String>, String> {
    platform::get(&key)
}

#[cfg(not(target_os = "android"))]
#[tauri::command]
pub fn remove_credential(key: String) -> Result<(), String> {
    platform::remove(&key)
}

#[cfg(target_os = "android")]
#[tauri::command]
pub fn store_credential(
    state: tauri::State<'_, CredentialStoreDir>,
    key: String,
    value: String,
) -> Result<(), String> {
    platform::store(&state.0, &key, &value)
}

#[cfg(target_os = "android")]
#[tauri::command]
pub fn get_credential(
    state: tauri::State<'_, CredentialStoreDir>,
    key: String,
) -> Result<Option<String>, String> {
    platform::get(&state.0, &key)
}

#[cfg(target_os = "android")]
#[tauri::command]
pub fn remove_credential(
    state: tauri::State<'_, CredentialStoreDir>,
    key: String,
) -> Result<(), String> {
    platform::remove(&state.0, &key)
}
