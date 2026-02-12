use crate::blob_store::BlobStore;
use crate::db::{AuthRepo, PublishedSiteInfo};
use crate::sync_v2::StorageCache;
use base64::Engine;
use diaryx_core::crdt::{BodyDocManager, WorkspaceCrdt, materialize_workspace};
use diaryx_core::fs::{RealFileSystem, SyncToAsyncFs};
use diaryx_core::publish::{PublishOptions, Publisher};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::warn;

pub type PublishLock = Arc<RwLock<HashSet<String>>>;
type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Serialize)]
pub struct AudienceBuild {
    pub name: String,
    pub file_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct PublishWorkspaceResult {
    pub slug: String,
    pub audiences: Vec<AudienceBuild>,
    pub published_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenClaims {
    #[serde(rename = "s")]
    pub slug: String,
    #[serde(rename = "a")]
    pub audience: String,
    #[serde(rename = "t")]
    pub token_id: String,
    #[serde(rename = "e")]
    pub expires_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SiteMeta {
    user_id: String,
    audiences: Vec<String>,
    revoked_tokens: Vec<String>,
    attachment_prefix: String,
}

pub fn new_publish_lock() -> PublishLock {
    Arc::new(RwLock::new(HashSet::new()))
}

pub async fn try_acquire_publish_lock(lock: &PublishLock, workspace_id: &str) -> bool {
    let mut guard = lock.write().await;
    if guard.contains(workspace_id) {
        return false;
    }
    guard.insert(workspace_id.to_string());
    true
}

pub async fn release_publish_lock(lock: &PublishLock, workspace_id: &str) {
    lock.write().await.remove(workspace_id);
}

pub async fn publish_workspace_to_r2(
    repo: &AuthRepo,
    storage_cache: &StorageCache,
    sites_store: &dyn BlobStore,
    attachments_store: &dyn BlobStore,
    workspace_id: &str,
    site: &PublishedSiteInfo,
) -> Result<PublishWorkspaceResult, String> {
    let storage = storage_cache
        .get_storage(workspace_id)
        .map_err(|e| format!("failed to open workspace storage: {}", e))?;

    let workspace_doc_name = format!("workspace:{}", workspace_id);
    let workspace = WorkspaceCrdt::load_with_name(storage.clone(), workspace_doc_name)
        .map_err(|e| format!("failed to load workspace CRDT: {}", e))?;
    let body_docs = BodyDocManager::new(storage);
    let mut files = materialize_workspace(&workspace, &body_docs, workspace_id).files;
    if files.is_empty() {
        return Err("workspace has no materialized markdown files".to_string());
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));

    let temp_dir = tempfile::tempdir().map_err(|e| format!("tempdir failed: {}", e))?;
    let workspace_dir = temp_dir.path().join("workspace");
    std::fs::create_dir_all(&workspace_dir)
        .map_err(|e| format!("failed to create workspace temp root: {}", e))?;

    let mut discovered_audiences: HashSet<String> = HashSet::new();
    let mut root_rel_path: Option<String> = None;

    for file in &files {
        if root_rel_path.is_none() && file.metadata.part_of.is_none() {
            root_rel_path = Some(file.path.clone());
        }

        if let Ok(parsed) = diaryx_core::frontmatter::parse_or_empty(&file.content)
            && let Some(audience) = parsed.frontmatter.get("audience")
        {
            match audience {
                serde_yaml::Value::String(s) => {
                    let value = s.trim().to_lowercase();
                    if !value.is_empty() && value != "private" {
                        discovered_audiences.insert(value);
                    }
                }
                serde_yaml::Value::Sequence(seq) => {
                    for entry in seq {
                        if let Some(s) = entry.as_str() {
                            let value = s.trim().to_lowercase();
                            if !value.is_empty() && value != "private" {
                                discovered_audiences.insert(value);
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        let target = workspace_dir.join(&file.path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create parent directories: {}", e))?;
        }
        std::fs::write(&target, &file.content)
            .map_err(|e| format!("failed to write materialized file {}: {}", file.path, e))?;
    }

    let root_rel_path = root_rel_path.unwrap_or_else(|| files[0].path.clone());
    let workspace_root = workspace_dir.join(root_rel_path);

    let attachment_map = repo
        .get_workspace_attachment_map(workspace_id)
        .map_err(|e| format!("failed to read workspace attachment map: {}", e))?;
    let mut attachment_hashes: HashMap<String, String> = HashMap::new();
    for (path, (hash, _mime_type)) in attachment_map {
        if let Some(normalized) = normalize_workspace_path(&path) {
            attachment_hashes.insert(normalized, hash);
        }
    }

    let mut audiences_to_build: Vec<String> = vec!["public".to_string()];
    let mut discovered: Vec<String> = discovered_audiences.into_iter().collect();
    discovered.sort();
    for audience in discovered {
        if audience != "public" {
            audiences_to_build.push(audience);
        }
    }

    let mut audience_builds = Vec::new();

    for audience in &audiences_to_build {
        let output_dir = temp_dir.path().join("builds").join(audience);
        std::fs::create_dir_all(&output_dir)
            .map_err(|e| format!("failed to create audience output dir: {}", e))?;

        let publisher = Publisher::new(SyncToAsyncFs::new(RealFileSystem));
        let options = PublishOptions {
            single_file: false,
            title: None,
            audience: if audience == "public" {
                None
            } else {
                Some(audience.clone())
            },
            force: true,
        };

        let result = publisher
            .publish(&workspace_root, &output_dir, &options)
            .await
            .map_err(|e| format!("publish failed for audience {}: {}", audience, e))?;

        for page in &result.pages {
            let html_path = output_dir.join(&page.dest_filename);
            let html = match std::fs::read_to_string(&html_path) {
                Ok(html) => html,
                Err(_) => continue,
            };
            let rewritten = rewrite_html_attachment_urls(
                &html,
                &page.source_path,
                &workspace_dir,
                &attachment_hashes,
                &site.slug,
                audience,
            );
            if rewritten != html {
                std::fs::write(&html_path, rewritten)
                    .map_err(|e| format!("failed to write rewritten html: {}", e))?;
            }
        }

        let mut file_count = 0usize;
        for entry in walkdir::WalkDir::new(&output_dir)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
        {
            let path = entry.path();
            let rel = path
                .strip_prefix(&output_dir)
                .map_err(|e| format!("failed to compute relative output path: {}", e))?;
            let rel = rel.to_string_lossy().replace('\\', "/");
            let key = format!("{}/{}/{}", site.slug, audience, rel);
            let bytes = std::fs::read(path)
                .map_err(|e| format!("failed to read rendered output {}: {}", rel, e))?;
            let mime_type = mime_guess::from_path(path)
                .first_or_octet_stream()
                .essence_str()
                .to_string();
            sites_store
                .put(&key, &bytes, &mime_type)
                .await
                .map_err(|e| format!("failed to upload {}: {}", key, e))?;
            file_count = file_count.saturating_add(1);
        }

        audience_builds.push(AudienceBuild {
            name: audience.clone(),
            file_count,
        });
    }

    let db_builds: Vec<(String, usize)> = audience_builds
        .iter()
        .map(|b| (b.name.clone(), b.file_count))
        .collect();
    repo.update_site_published(&site.id, &db_builds)
        .map_err(|e| format!("failed to update site build metadata: {}", e))?;

    write_site_meta(repo, sites_store, attachments_store, site).await?;

    Ok(PublishWorkspaceResult {
        slug: site.slug.clone(),
        audiences: audience_builds,
        published_at: chrono::Utc::now().timestamp(),
    })
}

pub async fn write_site_meta(
    repo: &AuthRepo,
    sites_store: &dyn BlobStore,
    attachments_store: &dyn BlobStore,
    site: &PublishedSiteInfo,
) -> Result<(), String> {
    let mut audiences: Vec<String> = repo
        .list_site_audience_builds(&site.id)
        .map_err(|e| format!("failed to read audience builds: {}", e))?
        .into_iter()
        .map(|b| b.audience)
        .collect();
    audiences.sort();
    audiences.dedup();
    if !audiences.iter().any(|a| a == "public") {
        audiences.insert(0, "public".to_string());
    }

    let revoked_tokens = repo
        .get_revoked_token_ids(&site.id)
        .map_err(|e| format!("failed to read revoked tokens: {}", e))?;

    let attachment_prefix = {
        let prefix = attachments_store.prefix().trim_matches('/');
        if prefix.is_empty() {
            format!("u/{}/blobs", site.user_id)
        } else {
            format!("{}/u/{}/blobs", prefix, site.user_id)
        }
    };

    let meta = SiteMeta {
        user_id: site.user_id.clone(),
        audiences,
        revoked_tokens,
        attachment_prefix,
    };

    let payload = serde_json::to_vec_pretty(&meta)
        .map_err(|e| format!("failed to serialize site metadata: {}", e))?;
    sites_store
        .put(
            &format!("{}/_meta.json", site.slug),
            &payload,
            "application/json",
        )
        .await
        .map_err(|e| format!("failed to upload site metadata: {}", e))
}

pub fn create_signed_token(
    signing_key: &[u8],
    slug: &str,
    audience: &str,
    token_id: &str,
    expires_at: Option<i64>,
) -> Result<String, String> {
    let claims = TokenClaims {
        slug: slug.to_string(),
        audience: audience.to_string(),
        token_id: token_id.to_string(),
        expires_at,
    };
    let payload = serde_json::to_vec(&claims)
        .map_err(|e| format!("failed to serialize token payload: {}", e))?;

    let mut mac =
        HmacSha256::new_from_slice(signing_key).map_err(|e| format!("invalid key: {}", e))?;
    mac.update(&payload);
    let signature = mac.finalize().into_bytes();

    let payload_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload);
    let signature_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature);
    Ok(format!("{}.{}", payload_b64, signature_b64))
}

pub fn validate_signed_token(signing_key: &[u8], token_string: &str) -> Option<TokenClaims> {
    let (payload_b64, signature_b64) = token_string.split_once('.')?;
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload_b64.as_bytes())
        .ok()?;
    let signature = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(signature_b64.as_bytes())
        .ok()?;

    let mut mac = HmacSha256::new_from_slice(signing_key).ok()?;
    mac.update(&payload);
    mac.verify_slice(&signature).ok()?;

    let claims: TokenClaims = serde_json::from_slice(&payload).ok()?;
    if claims.slug.trim().is_empty()
        || claims.audience.trim().is_empty()
        || claims.token_id.trim().is_empty()
    {
        return None;
    }

    if let Some(expires_at) = claims.expires_at
        && expires_at < chrono::Utc::now().timestamp()
    {
        return None;
    }

    Some(claims)
}

fn rewrite_html_attachment_urls(
    html: &str,
    source_path: &Path,
    workspace_dir: &Path,
    attachment_hashes: &HashMap<String, String>,
    slug: &str,
    audience: &str,
) -> String {
    let rewritten_src = rewrite_attribute_urls(
        html,
        "src=\"",
        source_path,
        workspace_dir,
        attachment_hashes,
        slug,
        audience,
    );

    rewrite_attribute_urls(
        &rewritten_src,
        "href=\"",
        source_path,
        workspace_dir,
        attachment_hashes,
        slug,
        audience,
    )
}

fn rewrite_attribute_urls(
    html: &str,
    marker: &str,
    source_path: &Path,
    workspace_dir: &Path,
    attachment_hashes: &HashMap<String, String>,
    slug: &str,
    audience: &str,
) -> String {
    let mut output = String::with_capacity(html.len());
    let mut remaining = html;

    while let Some(marker_pos) = remaining.find(marker) {
        let value_start = marker_pos + marker.len();
        output.push_str(&remaining[..value_start]);
        remaining = &remaining[value_start..];

        let Some(value_end) = remaining.find('"') else {
            output.push_str(remaining);
            return output;
        };

        let original_url = &remaining[..value_end];
        let rewritten_url = rewrite_single_url(
            original_url,
            source_path,
            workspace_dir,
            attachment_hashes,
            slug,
            audience,
        )
        .unwrap_or_else(|| original_url.to_string());

        output.push_str(&rewritten_url);
        remaining = &remaining[value_end..];
    }

    output.push_str(remaining);
    output
}

fn rewrite_single_url(
    url: &str,
    source_path: &Path,
    workspace_dir: &Path,
    attachment_hashes: &HashMap<String, String>,
    slug: &str,
    audience: &str,
) -> Option<String> {
    let trimmed = url.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('#')
        || trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("mailto:")
        || trimmed.starts_with("data:")
        || trimmed.starts_with("javascript:")
    {
        return None;
    }

    let source_rel = source_path.strip_prefix(workspace_dir).ok()?;
    let parsed = diaryx_core::link_parser::parse_link(trimmed);
    let canonical = diaryx_core::link_parser::to_canonical(&parsed, source_rel);
    let normalized = normalize_workspace_path(&canonical)?;

    let Some(hash) = attachment_hashes.get(&normalized) else {
        if normalized.contains("_attachments") {
            warn!(
                "publish rewrite: missing hash for attachment path {}",
                normalized
            );
        }
        return None;
    };

    let filename = PathBuf::from(&normalized)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file")
        .to_string();
    Some(format!("/{}/_a/{}/{}/{}", slug, audience, hash, filename))
}

fn normalize_workspace_path(path: &str) -> Option<String> {
    let mut normalized = PathBuf::new();

    for component in Path::new(path).components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::RootDir | Component::Prefix(_) => {}
        }
    }

    if normalized.as_os_str().is_empty() {
        None
    } else {
        Some(normalized.to_string_lossy().replace('\\', "/"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_round_trip_and_tamper_detection() {
        let key = [7u8; 32];
        let token = create_signed_token(&key, "demo", "family", "tok-1", None).unwrap();
        let claims = validate_signed_token(&key, &token).expect("claims");
        assert_eq!(claims.slug, "demo");
        assert_eq!(claims.audience, "family");

        let tampered = format!("{}x", token);
        assert!(validate_signed_token(&key, &tampered).is_none());
    }

    #[test]
    fn rewrites_attachment_urls_with_audience_scope() {
        let html = r#"<p><img src="_attachments/a.png" /><a href="_attachments/a.png">x</a></p>"#;
        let source = Path::new("/tmp/workspace/README.md");
        let workspace_dir = Path::new("/tmp/workspace");
        let mut map = HashMap::new();
        map.insert("_attachments/a.png".to_string(), "abc123".to_string());

        let rewritten =
            rewrite_html_attachment_urls(html, source, workspace_dir, &map, "my-site", "family");

        assert!(rewritten.contains("/my-site/_a/family/abc123/a.png"));
    }
}
