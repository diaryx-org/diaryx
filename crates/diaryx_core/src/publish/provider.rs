//! The `NamespaceProvider` port — the server-talking seam of the publish
//! pipeline. `diaryx_core::publish` orchestrates against this trait; the app
//! shells implement it (native via reqwest, web via fetch, and the legacy
//! Extism plugin via host functions).
//!
//! Methods return `Result<_, String>` to keep the port transport-agnostic;
//! callers surface the message. The async-trait bounds follow the core
//! convention: `Send` off on wasm32 (single-threaded JS hosts).

use async_trait::async_trait;

/// Minimal object metadata the publish diff needs: the key, its audience tag,
/// and the server-recorded content hash (`None` ⇒ treat as changed).
#[derive(Debug, Clone)]
pub struct ObjectMeta {
    pub key: String,
    pub audience: Option<String>,
    pub content_hash: Option<String>,
}

// Server operations the publish pipeline performs within a namespace. Defined
// twice (the core async-trait convention): `Send + Sync` on native so
// `&dyn NamespaceProvider` crosses threads; `?Send` on wasm32 where the host
// (Extism guest / browser) is single-threaded.

/// Server operations the publish pipeline performs within a namespace.
#[cfg(not(target_arch = "wasm32"))]
#[async_trait]
pub trait NamespaceProvider: Send + Sync {
    /// List all objects currently stored in the namespace (any audience).
    async fn list_objects(&self, ns_id: &str) -> Result<Vec<ObjectMeta>, String>;
    /// Upload an object, optionally registering its ARK. `object_key` is the key
    /// the ARK should resolve to (the server-rendered HTML) when it differs from
    /// the uploaded `key` (a markdown source); `None` ⇒ resolves to `key`.
    #[allow(clippy::too_many_arguments)]
    async fn put_object(
        &self,
        ns_id: &str,
        key: &str,
        bytes: &[u8],
        mime_type: &str,
        audience: Option<&str>,
        file_ark: Option<&str>,
        source_key: Option<&str>,
        object_key: Option<&str>,
        is_index: bool,
    ) -> Result<(), String>;
    /// Delete an object by key.
    async fn delete_object(&self, ns_id: &str, key: &str) -> Result<(), String>;
    /// Sync an audience's gate stack (`gates` = JSON array; empty = public).
    async fn sync_audience(
        &self,
        ns_id: &str,
        audience: &str,
        gates: &crate::yaml::Value,
    ) -> Result<(), String>;
    /// List the audiences the server currently has for the namespace.
    async fn list_audiences(&self, ns_id: &str) -> Result<Vec<String>, String>;
    /// Delete an audience (and its objects/tokens) from the server.
    async fn delete_audience(&self, ns_id: &str, audience: &str) -> Result<(), String>;
    /// Trigger the server-side render of the namespace's stored sources into HTML
    /// (ARK Layer 3). `base_url` is forwarded for canonical/sitemap/feeds.
    async fn build_namespace(&self, ns_id: &str, base_url: Option<&str>) -> Result<(), String>;
}

/// Server operations the publish pipeline performs within a namespace.
#[cfg(target_arch = "wasm32")]
#[async_trait(?Send)]
pub trait NamespaceProvider {
    /// List all objects currently stored in the namespace (any audience).
    async fn list_objects(&self, ns_id: &str) -> Result<Vec<ObjectMeta>, String>;
    /// Upload an object, optionally registering its ARK. `object_key` is the key
    /// the ARK should resolve to (the server-rendered HTML) when it differs from
    /// the uploaded `key` (a markdown source); `None` ⇒ resolves to `key`.
    #[allow(clippy::too_many_arguments)]
    async fn put_object(
        &self,
        ns_id: &str,
        key: &str,
        bytes: &[u8],
        mime_type: &str,
        audience: Option<&str>,
        file_ark: Option<&str>,
        source_key: Option<&str>,
        object_key: Option<&str>,
        is_index: bool,
    ) -> Result<(), String>;
    /// Delete an object by key.
    async fn delete_object(&self, ns_id: &str, key: &str) -> Result<(), String>;
    /// Sync an audience's gate stack (`gates` = JSON array; empty = public).
    async fn sync_audience(
        &self,
        ns_id: &str,
        audience: &str,
        gates: &crate::yaml::Value,
    ) -> Result<(), String>;
    /// List the audiences the server currently has for the namespace.
    async fn list_audiences(&self, ns_id: &str) -> Result<Vec<String>, String>;
    /// Delete an audience (and its objects/tokens) from the server.
    async fn delete_audience(&self, ns_id: &str, audience: &str) -> Result<(), String>;
    /// Trigger the server-side render of the namespace's stored sources into HTML
    /// (ARK Layer 3). `base_url` is forwarded for canonical/sitemap/feeds.
    async fn build_namespace(&self, ns_id: &str, base_url: Option<&str>) -> Result<(), String>;
}
