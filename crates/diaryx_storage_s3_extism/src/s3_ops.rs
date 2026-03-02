//! S3 REST API operations mapping to AsyncFileSystem methods.

use crate::host_bridge;
use crate::sigv4;
use std::collections::HashMap;

/// S3 configuration.
#[derive(serde::Serialize, serde::Deserialize, Clone, Default)]
pub struct S3Config {
    pub bucket: String,
    pub region: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    /// Optional prefix within the bucket (e.g., "diaryx/workspace1/").
    #[serde(default)]
    pub prefix: String,
    /// Optional custom endpoint for S3-compatible services (MinIO, R2, etc.).
    #[serde(default)]
    pub endpoint: Option<String>,
    /// Use path-style addressing instead of virtual-hosted style.
    #[serde(default)]
    pub path_style: bool,
}

impl S3Config {
    /// Build the base URL for S3 operations.
    fn base_url(&self) -> String {
        if let Some(endpoint) = &self.endpoint {
            let ep = endpoint.trim_end_matches('/');
            if self.path_style {
                format!("{ep}/{}", self.bucket)
            } else {
                // Virtual-hosted style with custom endpoint
                ep.to_string()
            }
        } else if self.path_style {
            format!("https://s3.{}.amazonaws.com/{}", self.region, self.bucket)
        } else {
            format!("https://{}.s3.{}.amazonaws.com", self.bucket, self.region)
        }
    }

    /// Build the full URL for a key (including prefix).
    fn object_url(&self, key: &str) -> String {
        let base = self.base_url();
        let full_key = self.full_key(key);
        format!("{base}/{full_key}")
    }

    /// Prepend the configured prefix to a key.
    fn full_key(&self, key: &str) -> String {
        if self.prefix.is_empty() {
            key.to_string()
        } else {
            let prefix = self.prefix.trim_end_matches('/');
            format!("{prefix}/{key}")
        }
    }

    /// Strip the prefix from a full key.
    fn strip_prefix<'a>(&self, full_key: &'a str) -> &'a str {
        if self.prefix.is_empty() {
            full_key
        } else {
            let prefix = self.prefix.trim_end_matches('/');
            let with_slash = format!("{prefix}/");
            full_key.strip_prefix(&with_slash).unwrap_or(full_key)
        }
    }

    fn sign_headers(
        &self,
        method: &str,
        url: &str,
        body_hash: &str,
        extra_headers: &HashMap<String, String>,
        timestamp: u64,
    ) -> HashMap<String, String> {
        sigv4::sign_request(
            method,
            url,
            extra_headers,
            body_hash,
            &self.region,
            "s3",
            &self.access_key_id,
            &self.secret_access_key,
            timestamp,
        )
    }
}

/// GET object — returns file content as string.
pub fn read_file(config: &S3Config, path: &str) -> Result<String, String> {
    let url = config.object_url(path);
    let timestamp = host_bridge::get_timestamp().map_err(|e| format!("timestamp: {e}"))?;
    let body_hash = sigv4::sha256_hex(b"");
    let headers = config.sign_headers("GET", &url, &body_hash, &HashMap::new(), timestamp);

    let resp = host_bridge::http_request(&url, "GET", &headers, None)?;
    if resp.status == 200 {
        Ok(resp.body)
    } else if resp.status == 404 {
        Err(format!("NotFound: {path}"))
    } else {
        Err(format!("S3 GET failed ({}): {}", resp.status, resp.body))
    }
}

/// GET object — returns file content as binary.
pub fn read_binary(config: &S3Config, path: &str) -> Result<Vec<u8>, String> {
    let url = config.object_url(path);
    let timestamp = host_bridge::get_timestamp().map_err(|e| format!("timestamp: {e}"))?;
    let body_hash = sigv4::sha256_hex(b"");
    let headers = config.sign_headers("GET", &url, &body_hash, &HashMap::new(), timestamp);

    let resp = host_bridge::http_request(&url, "GET", &headers, None)?;
    if resp.status == 200 {
        host_bridge::decode_response_body(&resp)
    } else if resp.status == 404 {
        Err(format!("NotFound: {path}"))
    } else {
        Err(format!("S3 GET failed ({}): {}", resp.status, resp.body))
    }
}

/// PUT object — write string content.
pub fn write_file(config: &S3Config, path: &str, content: &str) -> Result<(), String> {
    let url = config.object_url(path);
    let body = content.as_bytes();
    let timestamp = host_bridge::get_timestamp().map_err(|e| format!("timestamp: {e}"))?;
    let body_hash = sigv4::sha256_hex(body);

    let mut extra = HashMap::new();
    extra.insert(
        "content-type".to_string(),
        "text/plain; charset=utf-8".to_string(),
    );

    let headers = config.sign_headers("PUT", &url, &body_hash, &extra, timestamp);
    let mut all_headers = headers;
    all_headers.extend(extra);

    let resp = host_bridge::http_request_binary(&url, "PUT", &all_headers, body)?;
    if resp.status == 200 || resp.status == 204 {
        Ok(())
    } else {
        Err(format!("S3 PUT failed ({}): {}", resp.status, resp.body))
    }
}

/// PUT object — write binary content.
pub fn write_binary(config: &S3Config, path: &str, content: &[u8]) -> Result<(), String> {
    let url = config.object_url(path);
    let timestamp = host_bridge::get_timestamp().map_err(|e| format!("timestamp: {e}"))?;
    let body_hash = sigv4::sha256_hex(content);

    let mut extra = HashMap::new();
    extra.insert(
        "content-type".to_string(),
        "application/octet-stream".to_string(),
    );

    let headers = config.sign_headers("PUT", &url, &body_hash, &extra, timestamp);
    let mut all_headers = headers;
    all_headers.extend(extra);

    let resp = host_bridge::http_request_binary(&url, "PUT", &all_headers, content)?;
    if resp.status == 200 || resp.status == 204 {
        Ok(())
    } else {
        Err(format!(
            "S3 PUT binary failed ({}): {}",
            resp.status, resp.body
        ))
    }
}

/// DELETE object.
pub fn delete_file(config: &S3Config, path: &str) -> Result<(), String> {
    let url = config.object_url(path);
    let timestamp = host_bridge::get_timestamp().map_err(|e| format!("timestamp: {e}"))?;
    let body_hash = sigv4::sha256_hex(b"");
    let headers = config.sign_headers("DELETE", &url, &body_hash, &HashMap::new(), timestamp);

    let resp = host_bridge::http_request(&url, "DELETE", &headers, None)?;
    if resp.status == 204 || resp.status == 200 || resp.status == 404 {
        Ok(())
    } else {
        Err(format!("S3 DELETE failed ({}): {}", resp.status, resp.body))
    }
}

/// HEAD object — check if file exists.
pub fn exists(config: &S3Config, path: &str) -> Result<bool, String> {
    let url = config.object_url(path);
    let timestamp = host_bridge::get_timestamp().map_err(|e| format!("timestamp: {e}"))?;
    let body_hash = sigv4::sha256_hex(b"");
    let headers = config.sign_headers("HEAD", &url, &body_hash, &HashMap::new(), timestamp);

    let resp = host_bridge::http_request(&url, "HEAD", &headers, None)?;
    Ok(resp.status == 200)
}

/// HEAD object — get Last-Modified as milliseconds since epoch.
pub fn get_modified_time(config: &S3Config, path: &str) -> Result<Option<i64>, String> {
    let url = config.object_url(path);
    let timestamp = host_bridge::get_timestamp().map_err(|e| format!("timestamp: {e}"))?;
    let body_hash = sigv4::sha256_hex(b"");
    let headers = config.sign_headers("HEAD", &url, &body_hash, &HashMap::new(), timestamp);

    let resp = host_bridge::http_request(&url, "HEAD", &headers, None)?;
    if resp.status == 200 {
        // Try to parse Last-Modified header (we return the timestamp in ms)
        if let Some(last_modified) = resp.headers.get("last-modified") {
            // Basic HTTP date parsing — just return current timestamp as fallback
            let _ = last_modified;
            Ok(Some((timestamp as i64) * 1000))
        } else {
            Ok(Some((timestamp as i64) * 1000))
        }
    } else {
        Ok(None)
    }
}

/// LIST objects with prefix and delimiter.
pub fn list_files(config: &S3Config, dir: &str) -> Result<Vec<String>, String> {
    let prefix = config.full_key(dir.trim_end_matches('/'));
    let prefix_with_slash = if prefix.is_empty() {
        String::new()
    } else {
        format!("{prefix}/")
    };

    let base = config.base_url();
    let url = format!(
        "{base}?list-type=2&prefix={}&delimiter=/",
        uri_encode_param(&prefix_with_slash)
    );

    let timestamp = host_bridge::get_timestamp().map_err(|e| format!("timestamp: {e}"))?;
    let body_hash = sigv4::sha256_hex(b"");
    let headers = config.sign_headers("GET", &url, &body_hash, &HashMap::new(), timestamp);

    let resp = host_bridge::http_request(&url, "GET", &headers, None)?;
    if resp.status != 200 {
        return Err(format!("S3 LIST failed ({}): {}", resp.status, resp.body));
    }

    let mut files = Vec::new();

    // Parse <Key>...</Key> entries from XML response
    for key in extract_xml_values(&resp.body, "Key") {
        let stripped = config.strip_prefix(&key);
        // Remove the directory prefix to get just the filename
        let relative = stripped
            .strip_prefix(dir.trim_end_matches('/'))
            .unwrap_or(stripped)
            .trim_start_matches('/');
        if !relative.is_empty() && !relative.contains('/') {
            files.push(relative.to_string());
        }
    }

    // Parse <Prefix>...</Prefix> entries (subdirectories / common prefixes)
    for prefix_val in extract_xml_values(&resp.body, "Prefix") {
        let stripped = config.strip_prefix(&prefix_val);
        let relative = stripped
            .strip_prefix(dir.trim_end_matches('/'))
            .unwrap_or(stripped)
            .trim_start_matches('/')
            .trim_end_matches('/');
        if !relative.is_empty() && !relative.contains('/') {
            files.push(relative.to_string());
        }
    }

    Ok(files)
}

/// LIST objects with prefix, filtered to *.md files only.
pub fn list_md_files(config: &S3Config, dir: &str) -> Result<Vec<String>, String> {
    let all_files = list_files(config, dir)?;
    Ok(all_files
        .into_iter()
        .filter(|f| f.ends_with(".md"))
        .collect())
}

/// Check if a "directory" exists (any keys with that prefix).
pub fn is_dir(config: &S3Config, path: &str) -> Result<bool, String> {
    let prefix = config.full_key(path.trim_end_matches('/'));
    let prefix_with_slash = format!("{prefix}/");

    let base = config.base_url();
    let url = format!(
        "{base}?list-type=2&prefix={}&max-keys=1",
        uri_encode_param(&prefix_with_slash)
    );

    let timestamp = host_bridge::get_timestamp().map_err(|e| format!("timestamp: {e}"))?;
    let body_hash = sigv4::sha256_hex(b"");
    let headers = config.sign_headers("GET", &url, &body_hash, &HashMap::new(), timestamp);

    let resp = host_bridge::http_request(&url, "GET", &headers, None)?;
    if resp.status != 200 {
        return Ok(false);
    }

    // Check if any keys were returned
    Ok(resp.body.contains("<Key>") || resp.body.contains("<Prefix>"))
}

/// Move file: COPY + DELETE (S3 doesn't have native move).
pub fn move_file(config: &S3Config, from: &str, to: &str) -> Result<(), String> {
    let source_key = config.full_key(from);
    let dest_url = config.object_url(to);
    let timestamp = host_bridge::get_timestamp().map_err(|e| format!("timestamp: {e}"))?;
    let body_hash = sigv4::sha256_hex(b"");

    // COPY: PUT with x-amz-copy-source header
    let copy_source = format!("/{}/{source_key}", config.bucket);
    let mut extra = HashMap::new();
    extra.insert("x-amz-copy-source".to_string(), copy_source);

    let headers = config.sign_headers("PUT", &dest_url, &body_hash, &extra, timestamp);
    let mut all_headers = headers;
    all_headers.extend(extra);

    let resp = host_bridge::http_request(&dest_url, "PUT", &all_headers, None)?;
    if resp.status != 200 {
        return Err(format!("S3 COPY failed ({}): {}", resp.status, resp.body));
    }

    // DELETE the original
    delete_file(config, from)
}

/// HEAD bucket — test connection.
pub fn test_connection(config: &S3Config) -> Result<(), String> {
    let url = config.base_url();
    let timestamp = host_bridge::get_timestamp().map_err(|e| format!("timestamp: {e}"))?;
    let body_hash = sigv4::sha256_hex(b"");
    let headers = config.sign_headers("HEAD", &url, &body_hash, &HashMap::new(), timestamp);

    let resp = host_bridge::http_request(&url, "HEAD", &headers, None)?;
    if resp.status == 200 || resp.status == 301 {
        Ok(())
    } else if resp.status == 403 {
        Err("Access denied — check your credentials".to_string())
    } else if resp.status == 404 {
        Err("Bucket not found".to_string())
    } else {
        Err(format!(
            "S3 connection test failed ({}): {}",
            resp.status, resp.body
        ))
    }
}

/// Extract values between XML tags. Simple string-based parsing (no XML crate needed).
fn extract_xml_values(xml: &str, tag: &str) -> Vec<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut values = Vec::new();
    let mut search_from = 0;
    while let Some(start) = xml[search_from..].find(&open) {
        let abs_start = search_from + start + open.len();
        if let Some(end) = xml[abs_start..].find(&close) {
            values.push(xml[abs_start..abs_start + end].to_string());
            search_from = abs_start + end + close.len();
        } else {
            break;
        }
    }
    values
}

/// URI-encode a query parameter value.
fn uri_encode_param(s: &str) -> String {
    let mut encoded = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(b as char);
            }
            _ => {
                encoded.push_str(&format!("%{b:02X}"));
            }
        }
    }
    encoded
}
