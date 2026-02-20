//! Workspace naming, URL normalization, and publishing slug validation.
//!
//! Pure functions with no async or filesystem dependencies (WASM-safe).
//! These are the canonical implementations — frontend clients call through
//! the Command pattern rather than duplicating this logic.

/// Normalize a workspace name: trim whitespace and convert to lowercase.
///
/// Used for case-insensitive uniqueness comparisons.
pub fn normalize_workspace_name(name: &str) -> String {
    name.trim().to_lowercase()
}

/// Validate a workspace name for creation.
///
/// Checks that the name is non-empty after trimming and that it doesn't
/// collide with any existing local or (optionally) server workspace names.
/// Comparison is case-insensitive via [`normalize_workspace_name`].
///
/// Returns `Ok(trimmed_name)` on success, or `Err(message)` with a
/// human-readable reason on failure.
pub fn validate_workspace_name(
    name: &str,
    existing_local: &[String],
    existing_server: Option<&[String]>,
) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Please enter a workspace name".into());
    }

    let normalized = normalize_workspace_name(trimmed);

    if existing_local.iter().any(|n| normalize_workspace_name(n) == normalized) {
        return Err("A local workspace with that name already exists".into());
    }

    if let Some(server_names) = existing_server {
        if server_names
            .iter()
            .any(|n| normalize_workspace_name(n) == normalized)
        {
            return Err("A synced workspace with that name already exists".into());
        }
    }

    Ok(trimmed.to_string())
}

/// Validate a publishing site slug.
///
/// Must be 3–64 characters, lowercase letters, digits, or hyphens only.
pub fn validate_publishing_slug(slug: &str) -> Result<(), String> {
    let slug = slug.trim();
    if slug.len() < 3 || slug.len() > 64 {
        return Err("Use 3-64 lowercase letters, numbers, or hyphens.".into());
    }
    if !slug
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err("Use 3-64 lowercase letters, numbers, or hyphens.".into());
    }
    Ok(())
}

/// Normalize a server URL: trim whitespace and prepend `https://` if no scheme is present.
pub fn normalize_server_url(url: &str) -> String {
    let url = url.trim();
    if url.is_empty() {
        return String::new();
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        format!("https://{url}")
    } else {
        url.to_string()
    }
}

/// Convert an HTTP(S) URL to a WebSocket base URL.
///
/// Converts `https://` → `wss://` and `http://` → `ws://`, then strips
/// any trailing slashes and `/sync` or `/sync2` path suffixes.
pub fn to_websocket_base_url(http_url: &str) -> String {
    let url = http_url
        .replace("https://", "wss://")
        .replace("http://", "ws://");
    let url = url.trim_end_matches('/');
    let url = url
        .strip_suffix("/sync2")
        .or_else(|| url.strip_suffix("/sync"))
        .unwrap_or(url);
    url.trim_end_matches('/').to_string()
}

/// Convert an HTTP(S) URL to a WebSocket sync URL (appends `/sync2`).
///
/// Calls [`to_websocket_base_url`] then appends `/sync2`.
pub fn to_websocket_sync_url(http_url: &str) -> String {
    let base = to_websocket_base_url(http_url);
    format!("{base}/sync2")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── normalize_workspace_name ─────────────────────────────────────────

    #[test]
    fn normalize_trims_and_lowercases() {
        assert_eq!(normalize_workspace_name("  My Journal  "), "my journal");
    }

    #[test]
    fn normalize_empty() {
        assert_eq!(normalize_workspace_name(""), "");
        assert_eq!(normalize_workspace_name("   "), "");
    }

    // ── validate_workspace_name ──────────────────────────────────────────

    #[test]
    fn validate_empty_name() {
        let result = validate_workspace_name("", &[], None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("enter a workspace name"));
    }

    #[test]
    fn validate_whitespace_only() {
        let result = validate_workspace_name("   ", &[], None);
        assert!(result.is_err());
    }

    #[test]
    fn validate_unique_name() {
        let result = validate_workspace_name("New Journal", &[], None);
        assert_eq!(result.unwrap(), "New Journal");
    }

    #[test]
    fn validate_duplicate_local_case_insensitive() {
        let locals = vec!["my journal".to_string()];
        let result = validate_workspace_name("My Journal", &locals, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("local workspace"));
    }

    #[test]
    fn validate_duplicate_server() {
        let locals = vec![];
        let servers = vec!["Work Notes".to_string()];
        let result = validate_workspace_name("work notes", &locals, Some(&servers));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("synced workspace"));
    }

    #[test]
    fn validate_no_server_check_when_none() {
        let servers_exist = vec!["work notes".to_string()];
        // Without server check, it should pass even though server has the name
        let result = validate_workspace_name("work notes", &[], None);
        assert!(result.is_ok());
        // With server check, it should fail
        let result = validate_workspace_name("work notes", &[], Some(&servers_exist));
        assert!(result.is_err());
    }

    #[test]
    fn validate_trims_but_returns_original_casing() {
        let result = validate_workspace_name("  My Journal  ", &[], None);
        assert_eq!(result.unwrap(), "My Journal");
    }

    // ── validate_publishing_slug ─────────────────────────────────────────

    #[test]
    fn slug_valid() {
        assert!(validate_publishing_slug("my-site").is_ok());
        assert!(validate_publishing_slug("abc").is_ok());
        assert!(validate_publishing_slug("a-1").is_ok());
        assert!(validate_publishing_slug(&"a".repeat(64)).is_ok());
    }

    #[test]
    fn slug_too_short() {
        assert!(validate_publishing_slug("ab").is_err());
        assert!(validate_publishing_slug("").is_err());
    }

    #[test]
    fn slug_too_long() {
        assert!(validate_publishing_slug(&"a".repeat(65)).is_err());
    }

    #[test]
    fn slug_invalid_chars() {
        assert!(validate_publishing_slug("My-Site").is_err()); // uppercase
        assert!(validate_publishing_slug("my_site").is_err()); // underscore
        assert!(validate_publishing_slug("my site").is_err()); // space
        assert!(validate_publishing_slug("my.site").is_err()); // dot
    }

    // ── normalize_server_url ─────────────────────────────────────────────

    #[test]
    fn normalize_adds_https() {
        assert_eq!(normalize_server_url("example.com"), "https://example.com");
    }

    #[test]
    fn normalize_preserves_https() {
        assert_eq!(
            normalize_server_url("https://example.com"),
            "https://example.com"
        );
    }

    #[test]
    fn normalize_preserves_http() {
        assert_eq!(
            normalize_server_url("http://localhost:8080"),
            "http://localhost:8080"
        );
    }

    #[test]
    fn normalize_trims_whitespace() {
        assert_eq!(
            normalize_server_url("  example.com  "),
            "https://example.com"
        );
    }

    #[test]
    fn normalize_empty_url() {
        assert_eq!(normalize_server_url(""), "");
        assert_eq!(normalize_server_url("  "), "");
    }

    // ── to_websocket_base_url ────────────────────────────────────────────

    #[test]
    fn ws_base_converts_https() {
        assert_eq!(
            to_websocket_base_url("https://example.com"),
            "wss://example.com"
        );
    }

    #[test]
    fn ws_base_converts_http() {
        assert_eq!(
            to_websocket_base_url("http://localhost:8080"),
            "ws://localhost:8080"
        );
    }

    #[test]
    fn ws_base_strips_trailing_slash() {
        assert_eq!(
            to_websocket_base_url("https://example.com/"),
            "wss://example.com"
        );
    }

    #[test]
    fn ws_base_strips_sync2_suffix() {
        assert_eq!(
            to_websocket_base_url("https://example.com/sync2"),
            "wss://example.com"
        );
    }

    #[test]
    fn ws_base_strips_sync_suffix() {
        assert_eq!(
            to_websocket_base_url("https://example.com/sync"),
            "wss://example.com"
        );
    }

    // ── to_websocket_sync_url ────────────────────────────────────────────

    #[test]
    fn ws_sync_appends_sync2() {
        assert_eq!(
            to_websocket_sync_url("https://example.com"),
            "wss://example.com/sync2"
        );
    }

    #[test]
    fn ws_sync_idempotent_with_existing_sync2() {
        assert_eq!(
            to_websocket_sync_url("https://example.com/sync2"),
            "wss://example.com/sync2"
        );
    }

    #[test]
    fn ws_sync_replaces_sync_with_sync2() {
        assert_eq!(
            to_websocket_sync_url("https://example.com/sync"),
            "wss://example.com/sync2"
        );
    }

    #[test]
    fn ws_sync_with_port() {
        assert_eq!(
            to_websocket_sync_url("http://localhost:3000"),
            "ws://localhost:3000/sync2"
        );
    }
}
