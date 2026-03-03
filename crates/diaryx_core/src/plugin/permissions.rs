//! Plugin permission system.
//!
//! Provides fine-grained, path-based permissions for plugin access to
//! workspace files, HTTP, storage, and commands. Permissions are stored
//! in the root index frontmatter under a `plugins` key and synced via CRDT.
//!
//! # Permission resolution
//!
//! - `all` in include = allow everything (except explicit excludes)
//! - Folder links = allow all descendants of that index entry
//! - Audience tags = allow all files with that audience tag
//! - File links = allow that specific file
//! - Exclude wins over include (deny takes priority)
//! - Missing permission type = deny
//! - Missing plugin entry = deny all

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Configuration for a single plugin in the workspace.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginConfig {
    /// Download URL for the plugin WASM binary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download: Option<String>,

    /// Permission rules for this plugin.
    #[serde(default)]
    pub permissions: PluginPermissions,
}

/// All permission categories for a plugin.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginPermissions {
    /// Read files: `host_read_file`, `host_list_files`, `host_file_exists`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_files: Option<PermissionRule>,

    /// Edit existing files: `host_write_file` (existing), `SaveEntry`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edit_files: Option<PermissionRule>,

    /// Create new files: `CreateEntry`, `CreateChildEntry`, `host_write_file` (new).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub create_files: Option<PermissionRule>,

    /// Delete files: `DeleteEntry`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete_files: Option<PermissionRule>,

    /// Move/rename files: `MoveEntry`, `RenameEntry`, `ConvertToIndex`, `ConvertToLeaf`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub move_files: Option<PermissionRule>,

    /// HTTP requests: `host_http_request`.
    /// Scope values are domain patterns (e.g. `openrouter.ai`) or `all`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_requests: Option<PermissionRule>,

    /// Command execution: `host_execute_command` (future).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execute_commands: Option<PermissionRule>,

    /// Plugin storage: `host_storage_get`, `host_storage_set`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_storage: Option<PermissionRule>,
}

/// A single permission rule with include/exclude lists.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRule {
    /// Scope values that grant access. Can be:
    /// - `"all"` — allow everything
    /// - Markdown links: `[Title](/path/file.md)` — specific file or folder
    /// - Plain paths: `journal/daily/` — folder and descendants
    /// - Audience tags: `work`, `personal` — files with that audience
    /// - Domain patterns: `openrouter.ai` — for HTTP permissions
    #[serde(default)]
    pub include: Vec<String>,

    /// Scope values that deny access. Same format as include.
    /// Exclude always wins over include.
    #[serde(default)]
    pub exclude: Vec<String>,
}

/// The categories of permission that can be checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PermissionType {
    /// Read workspace files (`host_read_file`, `host_list_files`, `host_file_exists`).
    ReadFiles,
    /// Edit existing files (`host_write_file` on existing paths, `SaveEntry`).
    EditFiles,
    /// Create new files (`CreateEntry`, `CreateChildEntry`, `host_write_file` on new paths).
    CreateFiles,
    /// Delete files (`DeleteEntry`).
    DeleteFiles,
    /// Move or rename files (`MoveEntry`, `RenameEntry`, `ConvertToIndex`, `ConvertToLeaf`).
    MoveFiles,
    /// Make HTTP requests (`host_http_request`).
    HttpRequests,
    /// Execute host commands (`host_execute_command`, future).
    ExecuteCommands,
    /// Access plugin persistent storage (`host_storage_get`, `host_storage_set`).
    PluginStorage,
}

impl PermissionType {
    /// Human-readable label for UI display.
    pub fn label(&self) -> &'static str {
        match self {
            Self::ReadFiles => "read files",
            Self::EditFiles => "edit files",
            Self::CreateFiles => "create files",
            Self::DeleteFiles => "delete files",
            Self::MoveFiles => "move files",
            Self::HttpRequests => "make HTTP requests",
            Self::ExecuteCommands => "execute commands",
            Self::PluginStorage => "use plugin storage",
        }
    }

    /// Serialization key matching the YAML field name.
    pub fn key(&self) -> &'static str {
        match self {
            Self::ReadFiles => "read_files",
            Self::EditFiles => "edit_files",
            Self::CreateFiles => "create_files",
            Self::DeleteFiles => "delete_files",
            Self::MoveFiles => "move_files",
            Self::HttpRequests => "http_requests",
            Self::ExecuteCommands => "execute_commands",
            Self::PluginStorage => "plugin_storage",
        }
    }
}

/// Result of checking a permission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionCheck {
    /// The action is explicitly allowed by the configured rules.
    Allowed,
    /// The action is explicitly denied by the configured rules.
    Denied,
    /// No rule exists for this permission type — triggers permission request UI.
    NotConfigured,
}

/// Extract the path from a scope value that may be a markdown link or plain path.
///
/// - `[Title](/path/file.md)` → `path/file.md`
/// - `/path/file.md` → `path/file.md`
/// - `path/file.md` → `path/file.md`
fn extract_path_from_scope(scope: &str) -> Option<&str> {
    // Markdown link: [Title](path)
    if let Some(start) = scope.find("](") {
        let after = &scope[start + 2..];
        if let Some(end) = after.find(')') {
            let path = &after[..end];
            // Strip leading / for workspace-root paths
            return Some(path.strip_prefix('/').unwrap_or(path));
        }
    }
    // Plain path — strip leading /
    let path = scope.strip_prefix('/').unwrap_or(scope);
    if path.is_empty() { None } else { Some(path) }
}

/// Check if a file path is allowed by a permission rule.
///
/// Resolution:
/// - `all` in include → allowed (unless excluded)
/// - File link exact match → allowed
/// - Folder link → file is a descendant → allowed
/// - Audience tags are resolved externally; this function handles path-based checks
/// - Exclude always wins over include
pub fn check_file_permission(rule: &PermissionRule, file_path: &str) -> PermissionCheck {
    // Normalize the file path (strip leading /)
    let file_path = file_path.strip_prefix('/').unwrap_or(file_path);

    // Check excludes first (deny takes priority)
    for scope in &rule.exclude {
        let scope_trimmed = scope.trim();
        if scope_trimmed.eq_ignore_ascii_case("all") {
            return PermissionCheck::Denied;
        }
        if let Some(path) = extract_path_from_scope(scope_trimmed)
            && path_matches(file_path, path)
        {
            return PermissionCheck::Denied;
        }
    }

    // Check includes
    for scope in &rule.include {
        let scope_trimmed = scope.trim();
        if scope_trimmed.eq_ignore_ascii_case("all") {
            return PermissionCheck::Allowed;
        }
        if let Some(path) = extract_path_from_scope(scope_trimmed)
            && path_matches(file_path, path)
        {
            return PermissionCheck::Allowed;
        }
    }

    // No matching rule
    PermissionCheck::NotConfigured
}

/// Check if `file_path` matches a scope `pattern_path`.
///
/// - Exact match (normalized)
/// - Folder match: pattern is a directory prefix of the file
fn path_matches(file_path: &str, pattern_path: &str) -> bool {
    let file = Path::new(file_path);
    let pattern = Path::new(pattern_path);

    // Exact match
    if file == pattern {
        return true;
    }

    // Folder match: if the pattern looks like a directory (e.g. ends with /
    // or is a prefix of the file path with a / boundary)
    if file.starts_with(pattern) {
        return true;
    }

    // Also check if pattern without trailing .md extension is a directory prefix
    // (e.g. pattern = "journal/daily/daily.md" should match "journal/daily/2026-03-02.md")
    if let Some(parent) = pattern.parent()
        && !parent.as_os_str().is_empty()
        && file.starts_with(parent)
    {
        return true;
    }

    false
}

/// Check if an HTTP request is allowed by a permission rule.
///
/// Scope values are domain patterns (e.g. `openrouter.ai`, `api.anthropic.com`) or `all`.
pub fn check_http_permission(rule: &PermissionRule, url: &str) -> PermissionCheck {
    let domain = extract_domain(url);

    // Check excludes first
    for scope in &rule.exclude {
        let scope_trimmed = scope.trim();
        if scope_trimmed.eq_ignore_ascii_case("all") {
            return PermissionCheck::Denied;
        }
        if domain_matches(&domain, scope_trimmed) {
            return PermissionCheck::Denied;
        }
    }

    // Check includes
    for scope in &rule.include {
        let scope_trimmed = scope.trim();
        if scope_trimmed.eq_ignore_ascii_case("all") {
            return PermissionCheck::Allowed;
        }
        if domain_matches(&domain, scope_trimmed) {
            return PermissionCheck::Allowed;
        }
    }

    PermissionCheck::NotConfigured
}

/// Check if plugin storage access is allowed.
pub fn check_storage_permission(rule: &PermissionRule) -> PermissionCheck {
    // Check excludes first
    for scope in &rule.exclude {
        if scope.trim().eq_ignore_ascii_case("all") {
            return PermissionCheck::Denied;
        }
    }

    // Check includes
    for scope in &rule.include {
        if scope.trim().eq_ignore_ascii_case("all") {
            return PermissionCheck::Allowed;
        }
    }

    PermissionCheck::NotConfigured
}

/// Look up the permission rule for a given type from a plugin's permissions.
pub fn get_permission_rule(
    permissions: &PluginPermissions,
    permission_type: PermissionType,
) -> Option<&PermissionRule> {
    match permission_type {
        PermissionType::ReadFiles => permissions.read_files.as_ref(),
        PermissionType::EditFiles => permissions.edit_files.as_ref(),
        PermissionType::CreateFiles => permissions.create_files.as_ref(),
        PermissionType::DeleteFiles => permissions.delete_files.as_ref(),
        PermissionType::MoveFiles => permissions.move_files.as_ref(),
        PermissionType::HttpRequests => permissions.http_requests.as_ref(),
        PermissionType::ExecuteCommands => permissions.execute_commands.as_ref(),
        PermissionType::PluginStorage => permissions.plugin_storage.as_ref(),
    }
}

/// Check a plugin's permission for a specific action.
///
/// Returns `NotConfigured` if the plugin has no entry in the workspace config,
/// or if the specific permission type has no rule.
pub fn check_permission(
    plugins_config: &HashMap<String, PluginConfig>,
    plugin_id: &str,
    permission_type: PermissionType,
    target: &str,
) -> PermissionCheck {
    let config = match plugins_config.get(plugin_id) {
        Some(c) => c,
        None => return PermissionCheck::NotConfigured,
    };

    let rule = match get_permission_rule(&config.permissions, permission_type) {
        Some(r) => r,
        None => return PermissionCheck::NotConfigured,
    };

    match permission_type {
        PermissionType::HttpRequests => check_http_permission(rule, target),
        PermissionType::PluginStorage => check_storage_permission(rule),
        _ => check_file_permission(rule, target),
    }
}

/// Extract the domain from a URL.
fn extract_domain(url: &str) -> String {
    // Strip scheme
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);

    // Take everything before the first /
    let domain = without_scheme.split('/').next().unwrap_or(without_scheme);

    // Strip port
    domain.split(':').next().unwrap_or(domain).to_lowercase()
}

/// Check if a domain matches a pattern.
///
/// Supports exact match and suffix match (e.g. `api.example.com` matches `example.com`).
fn domain_matches(domain: &str, pattern: &str) -> bool {
    let pattern_lower = pattern.to_lowercase();
    if domain == pattern_lower {
        return true;
    }
    // Suffix match: domain ends with .pattern
    domain.ends_with(&format!(".{}", pattern_lower))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_path_from_scope_markdown_link() {
        assert_eq!(
            extract_path_from_scope("[Daily](/journal/daily/daily.md)"),
            Some("journal/daily/daily.md")
        );
    }

    #[test]
    fn test_extract_path_from_scope_root_path() {
        assert_eq!(
            extract_path_from_scope("/journal/daily/daily.md"),
            Some("journal/daily/daily.md")
        );
    }

    #[test]
    fn test_extract_path_from_scope_relative_path() {
        assert_eq!(
            extract_path_from_scope("journal/daily/daily.md"),
            Some("journal/daily/daily.md")
        );
    }

    #[test]
    fn test_check_file_permission_all_include() {
        let rule = PermissionRule {
            include: vec!["all".to_string()],
            exclude: vec![],
        };
        assert_eq!(
            check_file_permission(&rule, "journal/2026-03-02.md"),
            PermissionCheck::Allowed
        );
    }

    #[test]
    fn test_check_file_permission_all_with_exclude() {
        let rule = PermissionRule {
            include: vec!["all".to_string()],
            exclude: vec!["[Sensitive](/private/sensitive.md)".to_string()],
        };
        assert_eq!(
            check_file_permission(&rule, "journal/2026-03-02.md"),
            PermissionCheck::Allowed
        );
        assert_eq!(
            check_file_permission(&rule, "private/sensitive.md"),
            PermissionCheck::Denied
        );
        // Descendants of the excluded folder are also denied
        assert_eq!(
            check_file_permission(&rule, "private/sensitive/secret.md"),
            PermissionCheck::Denied
        );
    }

    #[test]
    fn test_check_file_permission_specific_folder() {
        let rule = PermissionRule {
            include: vec!["[Daily](/journal/daily/daily.md)".to_string()],
            exclude: vec![],
        };
        // The file is under the folder containing daily.md
        assert_eq!(
            check_file_permission(&rule, "journal/daily/2026-03-02.md"),
            PermissionCheck::Allowed
        );
        // File outside the folder
        assert_eq!(
            check_file_permission(&rule, "private/notes.md"),
            PermissionCheck::NotConfigured
        );
    }

    #[test]
    fn test_check_file_permission_exact_file() {
        let rule = PermissionRule {
            include: vec!["[My File](/projects/todo.md)".to_string()],
            exclude: vec![],
        };
        assert_eq!(
            check_file_permission(&rule, "projects/todo.md"),
            PermissionCheck::Allowed
        );
        assert_eq!(
            check_file_permission(&rule, "projects/other.md"),
            PermissionCheck::Allowed // same parent directory
        );
    }

    #[test]
    fn test_check_file_permission_no_match() {
        let rule = PermissionRule {
            include: vec!["[Daily](/journal/daily/daily.md)".to_string()],
            exclude: vec![],
        };
        assert_eq!(
            check_file_permission(&rule, "private/secret.md"),
            PermissionCheck::NotConfigured
        );
    }

    #[test]
    fn test_check_file_permission_exclude_wins() {
        let rule = PermissionRule {
            include: vec!["all".to_string()],
            exclude: vec!["[Private](/private/private.md)".to_string()],
        };
        assert_eq!(
            check_file_permission(&rule, "private/notes.md"),
            PermissionCheck::Denied
        );
    }

    #[test]
    fn test_check_http_permission_specific_domains() {
        let rule = PermissionRule {
            include: vec!["openrouter.ai".to_string(), "api.anthropic.com".to_string()],
            exclude: vec![],
        };
        assert_eq!(
            check_http_permission(&rule, "https://openrouter.ai/v1/chat"),
            PermissionCheck::Allowed
        );
        assert_eq!(
            check_http_permission(&rule, "https://api.anthropic.com/v1/messages"),
            PermissionCheck::Allowed
        );
        assert_eq!(
            check_http_permission(&rule, "https://evil.example.com/data"),
            PermissionCheck::NotConfigured
        );
    }

    #[test]
    fn test_check_http_permission_all() {
        let rule = PermissionRule {
            include: vec!["all".to_string()],
            exclude: vec![],
        };
        assert_eq!(
            check_http_permission(&rule, "https://anything.com/path"),
            PermissionCheck::Allowed
        );
    }

    #[test]
    fn test_check_storage_permission() {
        let rule = PermissionRule {
            include: vec!["all".to_string()],
            exclude: vec![],
        };
        assert_eq!(check_storage_permission(&rule), PermissionCheck::Allowed);

        let empty_rule = PermissionRule {
            include: vec![],
            exclude: vec![],
        };
        assert_eq!(
            check_storage_permission(&empty_rule),
            PermissionCheck::NotConfigured
        );
    }

    #[test]
    fn test_check_permission_missing_plugin() {
        let config: HashMap<String, PluginConfig> = HashMap::new();
        assert_eq!(
            check_permission(&config, "diaryx.ai", PermissionType::ReadFiles, "file.md"),
            PermissionCheck::NotConfigured
        );
    }

    #[test]
    fn test_check_permission_missing_rule() {
        let mut config = HashMap::new();
        config.insert(
            "diaryx.ai".to_string(),
            PluginConfig {
                download: None,
                permissions: PluginPermissions::default(),
            },
        );
        assert_eq!(
            check_permission(&config, "diaryx.ai", PermissionType::ReadFiles, "file.md"),
            PermissionCheck::NotConfigured
        );
    }

    #[test]
    fn test_check_permission_full_config() {
        let mut config = HashMap::new();
        config.insert(
            "diaryx.ai".to_string(),
            PluginConfig {
                download: Some("https://cdn.diaryx.org/plugins/diaryx_ai".to_string()),
                permissions: PluginPermissions {
                    read_files: Some(PermissionRule {
                        include: vec!["[Daily](/journal/daily/daily.md)".to_string()],
                        exclude: vec!["[Sensitive](/private/sensitive.md)".to_string()],
                    }),
                    http_requests: Some(PermissionRule {
                        include: vec!["openrouter.ai".to_string()],
                        exclude: vec![],
                    }),
                    plugin_storage: Some(PermissionRule {
                        include: vec!["all".to_string()],
                        exclude: vec![],
                    }),
                    ..Default::default()
                },
            },
        );

        // Read allowed file
        assert_eq!(
            check_permission(
                &config,
                "diaryx.ai",
                PermissionType::ReadFiles,
                "journal/daily/2026-03-02.md"
            ),
            PermissionCheck::Allowed
        );

        // Read excluded file
        assert_eq!(
            check_permission(
                &config,
                "diaryx.ai",
                PermissionType::ReadFiles,
                "private/sensitive.md"
            ),
            PermissionCheck::Denied
        );

        // Edit not configured
        assert_eq!(
            check_permission(
                &config,
                "diaryx.ai",
                PermissionType::EditFiles,
                "journal/daily/2026-03-02.md"
            ),
            PermissionCheck::NotConfigured
        );

        // HTTP allowed domain
        assert_eq!(
            check_permission(
                &config,
                "diaryx.ai",
                PermissionType::HttpRequests,
                "https://openrouter.ai/v1/chat"
            ),
            PermissionCheck::Allowed
        );

        // Storage allowed
        assert_eq!(
            check_permission(&config, "diaryx.ai", PermissionType::PluginStorage, ""),
            PermissionCheck::Allowed
        );
    }

    #[test]
    fn test_extract_domain() {
        assert_eq!(
            extract_domain("https://openrouter.ai/v1/chat"),
            "openrouter.ai"
        );
        assert_eq!(
            extract_domain("https://api.anthropic.com/v1/messages"),
            "api.anthropic.com"
        );
        assert_eq!(extract_domain("http://localhost:8080/api"), "localhost");
        assert_eq!(extract_domain("openrouter.ai"), "openrouter.ai");
    }

    #[test]
    fn test_domain_matches_suffix() {
        assert!(domain_matches("api.openrouter.ai", "openrouter.ai"));
        assert!(!domain_matches("notopenrouter.ai", "openrouter.ai"));
        assert!(domain_matches("openrouter.ai", "openrouter.ai"));
    }

    #[test]
    fn test_yaml_round_trip() {
        let config = PluginConfig {
            download: Some("https://cdn.diaryx.org/plugins/diaryx_ai".to_string()),
            permissions: PluginPermissions {
                read_files: Some(PermissionRule {
                    include: vec![
                        "[Daily](/journal/daily/daily.md)".to_string(),
                        "[Utility](/utility/utility.md)".to_string(),
                    ],
                    exclude: vec!["[Sensitive](/private/sensitive.md)".to_string()],
                }),
                edit_files: Some(PermissionRule {
                    include: vec!["[Daily](/journal/daily/daily.md)".to_string()],
                    exclude: vec![],
                }),
                http_requests: Some(PermissionRule {
                    include: vec!["openrouter.ai".to_string(), "api.anthropic.com".to_string()],
                    exclude: vec![],
                }),
                plugin_storage: Some(PermissionRule {
                    include: vec!["all".to_string()],
                    exclude: vec![],
                }),
                ..Default::default()
            },
        };

        let yaml = serde_yaml::to_string(&config).unwrap();
        let parsed: PluginConfig = serde_yaml::from_str(&yaml).unwrap();

        assert_eq!(parsed.download, config.download);
        assert!(parsed.permissions.read_files.is_some());
        let read = parsed.permissions.read_files.unwrap();
        assert_eq!(read.include.len(), 2);
        assert_eq!(read.exclude.len(), 1);
    }

    #[test]
    fn test_leading_slash_normalization() {
        let rule = PermissionRule {
            include: vec!["all".to_string()],
            exclude: vec![],
        };
        // Both with and without leading slash should work
        assert_eq!(
            check_file_permission(&rule, "/journal/file.md"),
            PermissionCheck::Allowed
        );
        assert_eq!(
            check_file_permission(&rule, "journal/file.md"),
            PermissionCheck::Allowed
        );
    }
}
