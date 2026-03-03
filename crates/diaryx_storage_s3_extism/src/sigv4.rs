//! AWS Signature V4 signing for S3 requests.
//!
//! Pure Rust implementation using `hmac` + `sha2` crates (WASM-compatible).
//! Supports custom endpoints (MinIO, Cloudflare R2, etc.).

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

type HmacSha256 = Hmac<Sha256>;

/// Sign an S3 request using AWS Signature V4.
///
/// Returns headers that must be added to the request (Authorization, x-amz-date,
/// x-amz-content-sha256, and optionally Host).
pub fn sign_request(
    method: &str,
    url: &str,
    headers: &HashMap<String, String>,
    body_hash: &str,
    region: &str,
    service: &str,
    access_key: &str,
    secret_key: &str,
    timestamp_secs: u64,
) -> HashMap<String, String> {
    let datetime = format_datetime(timestamp_secs);
    let date = &datetime[..8];

    // Parse URL components
    let (host, path, query) = parse_url(url);

    // Build canonical headers (must include host, x-amz-content-sha256, x-amz-date)
    let mut canonical_headers: Vec<(String, String)> = vec![
        ("host".to_string(), host.clone()),
        ("x-amz-content-sha256".to_string(), body_hash.to_string()),
        ("x-amz-date".to_string(), datetime.clone()),
    ];

    // Add any additional headers
    for (k, v) in headers {
        let lower = k.to_lowercase();
        if lower != "host"
            && lower != "x-amz-content-sha256"
            && lower != "x-amz-date"
            && lower != "authorization"
        {
            canonical_headers.push((lower, v.trim().to_string()));
        }
    }
    canonical_headers.sort_by(|a, b| a.0.cmp(&b.0));

    let signed_headers: Vec<&str> = canonical_headers.iter().map(|(k, _)| k.as_str()).collect();
    let signed_headers_str = signed_headers.join(";");

    let canonical_headers_str: String = canonical_headers
        .iter()
        .map(|(k, v)| format!("{k}:{v}\n"))
        .collect();

    // Canonical request
    let canonical_request = format!(
        "{method}\n{path}\n{query}\n{canonical_headers_str}\n{signed_headers_str}\n{body_hash}"
    );

    let canonical_request_hash = sha256_hex(canonical_request.as_bytes());

    // String to sign
    let credential_scope = format!("{date}/{region}/{service}/aws4_request");
    let string_to_sign =
        format!("AWS4-HMAC-SHA256\n{datetime}\n{credential_scope}\n{canonical_request_hash}");

    // Signing key
    let signing_key = derive_signing_key(secret_key, date, region, service);

    // Signature
    let signature = hmac_sha256_hex(&signing_key, string_to_sign.as_bytes());

    // Authorization header
    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={access_key}/{credential_scope}, SignedHeaders={signed_headers_str}, Signature={signature}"
    );

    let mut result = HashMap::new();
    result.insert("Authorization".to_string(), authorization);
    result.insert("x-amz-date".to_string(), datetime);
    result.insert("x-amz-content-sha256".to_string(), body_hash.to_string());
    result.insert("Host".to_string(), host);
    result
}

/// Compute SHA-256 hash of data, returned as lowercase hex.
pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// Format a Unix timestamp as ISO 8601 date-time (YYYYMMDD'T'HHMMSS'Z').
fn format_datetime(timestamp_secs: u64) -> String {
    let secs = timestamp_secs;
    // Simple UTC datetime calculation
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    let (year, month, day) = days_to_ymd(days);

    format!("{year:04}{month:02}{day:02}T{hours:02}{minutes:02}{seconds:02}Z")
}

/// Convert days since Unix epoch to (year, month, day).
fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Derive the signing key for AWS Signature V4.
fn derive_signing_key(secret_key: &str, date: &str, region: &str, service: &str) -> Vec<u8> {
    let k_date = hmac_sha256(format!("AWS4{secret_key}").as_bytes(), date.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, service.as_bytes());
    hmac_sha256(&k_service, b"aws4_request")
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn hmac_sha256_hex(key: &[u8], data: &[u8]) -> String {
    hex::encode(hmac_sha256(key, data))
}

/// Parse a URL into (host, path, query_string).
fn parse_url(url: &str) -> (String, String, String) {
    // Strip scheme
    let without_scheme = if let Some(rest) = url.strip_prefix("https://") {
        rest
    } else if let Some(rest) = url.strip_prefix("http://") {
        rest
    } else {
        url
    };

    // Split host from path
    let (host_port, path_query) = match without_scheme.find('/') {
        Some(i) => (&without_scheme[..i], &without_scheme[i..]),
        None => (without_scheme, "/"),
    };

    // Split path from query
    let (path, query) = match path_query.find('?') {
        Some(i) => (&path_query[..i], &path_query[i + 1..]),
        None => (path_query, ""),
    };

    // URI-encode path segments (but keep /)
    let encoded_path = encode_path(path);

    (
        host_port.to_string(),
        encoded_path,
        sort_query_string(query),
    )
}

/// URI-encode path, preserving `/` separators.
fn encode_path(path: &str) -> String {
    path.split('/')
        .map(uri_encode)
        .collect::<Vec<_>>()
        .join("/")
}

/// Sort query string parameters for canonical request.
fn sort_query_string(query: &str) -> String {
    if query.is_empty() {
        return String::new();
    }
    let mut pairs: Vec<&str> = query.split('&').collect();
    pairs.sort();
    pairs.join("&")
}

/// URI-encode a string per AWS rules (RFC 3986, except `/` is encoded).
fn uri_encode(s: &str) -> String {
    let mut encoded = String::with_capacity(s.len());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_hex() {
        let hash = sha256_hex(b"");
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_format_datetime() {
        // 2024-01-15T11:30:45Z = 1705318245
        let dt = format_datetime(1705318245);
        assert_eq!(dt, "20240115T113045Z");
    }

    #[test]
    fn test_parse_url() {
        let (host, path, query) =
            parse_url("https://my-bucket.s3.us-east-1.amazonaws.com/my/key.txt");
        assert_eq!(host, "my-bucket.s3.us-east-1.amazonaws.com");
        assert_eq!(path, "/my/key.txt");
        assert_eq!(query, "");
    }

    #[test]
    fn test_parse_url_with_query() {
        let (host, path, query) =
            parse_url("https://s3.amazonaws.com/bucket?list-type=2&prefix=foo/");
        assert_eq!(host, "s3.amazonaws.com");
        assert_eq!(path, "/bucket");
        assert_eq!(query, "list-type=2&prefix=foo/");
    }
}
