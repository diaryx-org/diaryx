//! Host function imports for the templating Extism guest.

use extism_pdk::*;

#[host_fn]
extern "ExtismHost" {
    pub fn host_log(input: String) -> String;
    pub fn host_read_file(input: String) -> String;
    pub fn host_list_files(input: String) -> String;
    pub fn host_file_exists(input: String) -> String;
    pub fn host_write_file(input: String) -> String;
    pub fn host_delete_file(input: String) -> String;
    pub fn host_storage_get(input: String) -> String;
    pub fn host_storage_set(input: String) -> String;
}

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

pub fn log_message(level: &str, message: &str) {
    let input = serde_json::json!({ "level": level, "message": message }).to_string();
    let _ = unsafe { host_log(input) };
}

pub fn read_file(path: &str) -> Result<String, String> {
    let input = serde_json::json!({ "path": path }).to_string();
    unsafe { host_read_file(input) }.map_err(|e| format!("host_read_file failed: {e}"))
}

pub fn list_files(prefix: &str) -> Result<Vec<String>, String> {
    let input = serde_json::json!({ "prefix": prefix }).to_string();
    let result =
        unsafe { host_list_files(input) }.map_err(|e| format!("host_list_files failed: {e}"))?;
    serde_json::from_str(&result).map_err(|e| format!("Failed to parse file list: {e}"))
}

pub fn file_exists(path: &str) -> Result<bool, String> {
    let input = serde_json::json!({ "path": path }).to_string();
    let result =
        unsafe { host_file_exists(input) }.map_err(|e| format!("host_file_exists failed: {e}"))?;

    if result == "true" {
        return Ok(true);
    }
    if result == "false" {
        return Ok(false);
    }

    serde_json::from_str(&result).map_err(|e| format!("Failed to parse exists result: {e}"))
}

pub fn write_file(path: &str, content: &str) -> Result<(), String> {
    let input = serde_json::json!({ "path": path, "content": content }).to_string();
    unsafe { host_write_file(input) }.map_err(|e| format!("host_write_file failed: {e}"))?;
    Ok(())
}

pub fn delete_file(path: &str) -> Result<(), String> {
    let input = serde_json::json!({ "path": path }).to_string();
    unsafe { host_delete_file(input) }.map_err(|e| format!("host_delete_file failed: {e}"))?;
    Ok(())
}

pub fn storage_get(key: &str) -> Result<Option<Vec<u8>>, String> {
    let input = serde_json::json!({ "key": key }).to_string();
    let result =
        unsafe { host_storage_get(input) }.map_err(|e| format!("host_storage_get failed: {e}"))?;
    if result.is_empty() {
        return Ok(None);
    }

    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(&result) {
        if let Some(data_str) = obj.get("data").and_then(|v| v.as_str()) {
            if data_str.is_empty() {
                return Ok(None);
            }
            let bytes = BASE64
                .decode(data_str)
                .map_err(|e| format!("Failed to decode storage data: {e}"))?;
            return Ok(Some(bytes));
        }
        if obj.is_null() {
            return Ok(None);
        }
    }

    let bytes = BASE64
        .decode(&result)
        .map_err(|e| format!("Failed to decode storage data: {e}"))?;
    Ok(Some(bytes))
}

pub fn storage_set(key: &str, data: &[u8]) -> Result<(), String> {
    let encoded = BASE64.encode(data);
    let input = serde_json::json!({ "key": key, "data": encoded }).to_string();
    unsafe { host_storage_set(input) }.map_err(|e| format!("host_storage_set failed: {e}"))?;
    Ok(())
}
