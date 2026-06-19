//! Server-side render-on-publish (ARK Layer 3, Phase 2).
//!
//! Reconstructs and renders a namespace's site from the markdown **sources** the
//! client uploaded, then stores the rendered HTML + static assets and (re)points
//! the object store at them. The client no longer pushes pre-rendered HTML; it
//! uploads sources, registers ARKs, and calls the build endpoint, which runs
//! this service.
//!
//! Sources are keyed by their (sanitized) workspace-relative path; the
//! per-audience root is the page whose dest is `index.html`. Rendering itself
//! lives in the portable `diaryx_render` engine.

use std::collections::BTreeMap;

use diaryx_render::SiteStyle;
use diaryx_render::site::{SiteOptions, SourceDoc, render_site};

use crate::domain::ArkIndexEntry;
use crate::ports::{ArkIndexStore, BlobStore, NamespaceStore, ObjectMetaStore, ServerCoreError};
use crate::use_cases::ark::ARK_WORKSPACE_INDEX;
use crate::use_cases::objects::ObjectService;

/// Summary of a build run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuildSummary {
    /// Number of audiences rendered.
    pub audiences: usize,
    /// Number of HTML pages written.
    pub pages_rendered: usize,
    /// Number of static/supplementary assets written.
    pub assets_written: usize,
}

/// Renders a namespace's stored sources into HTML on the server.
pub struct RenderService<'a> {
    namespace_store: &'a dyn NamespaceStore,
    object_meta_store: &'a dyn ObjectMetaStore,
    blob_store: &'a dyn BlobStore,
    ark_index: &'a dyn ArkIndexStore,
}

impl<'a> RenderService<'a> {
    pub fn new(
        namespace_store: &'a dyn NamespaceStore,
        object_meta_store: &'a dyn ObjectMetaStore,
        blob_store: &'a dyn BlobStore,
        ark_index: &'a dyn ArkIndexStore,
    ) -> Self {
        Self {
            namespace_store,
            object_meta_store,
            blob_store,
            ark_index,
        }
    }

    /// Render every page in the namespace (grouped by audience) from its stored
    /// markdown source and write the resulting HTML + assets. Owner-gated.
    pub async fn build_namespace(
        &self,
        namespace_id: &str,
        caller_user_id: &str,
        base_url: Option<&str>,
    ) -> Result<BuildSummary, ServerCoreError> {
        // Ownership check (the build mutates the namespace's objects).
        let ns = self
            .namespace_store
            .get_namespace(namespace_id)
            .await?
            .ok_or_else(|| ServerCoreError::not_found("Namespace not found"))?;
        if ns.owner_user_id != caller_user_id {
            return Err(ServerCoreError::permission_denied(
                "You do not own this namespace",
            ));
        }

        let rows = self.ark_index.list_ark_entries(namespace_id).await?;

        // Group renderable page rows by audience. Skip the workspace-index
        // sentinel and any row without a source (nothing to render from).
        let mut by_audience: BTreeMap<String, Vec<&ArkIndexEntry>> = BTreeMap::new();
        for row in &rows {
            if row.file_ark == ARK_WORKSPACE_INDEX {
                continue;
            }
            if row.source_key.is_none() {
                continue;
            }
            let aud = row.audience.clone().unwrap_or_default();
            by_audience.entry(aud).or_default().push(row);
        }

        let object_service = ObjectService::new(
            self.namespace_store,
            self.object_meta_store,
            self.blob_store,
        );

        let mut summary = BuildSummary::default();

        for (audience, page_rows) in by_audience {
            let aud_prefix = format!("{}/", audience);

            // Build the source set for this audience.
            let mut sources: Vec<SourceDoc> = Vec::with_capacity(page_rows.len());
            for row in &page_rows {
                let source_key = row.source_key.as_deref().unwrap();
                let bytes = match self
                    .fetch_source(namespace_id, source_key, caller_user_id)
                    .await?
                {
                    Some(b) => b,
                    None => continue,
                };
                let markdown = String::from_utf8_lossy(&bytes).into_owned();

                // Canonical workspace path = source key minus the audience prefix.
                let path = strip_prefix(source_key, &aud_prefix).to_string();
                // Per-audience root = the page whose dest is index.html.
                let dest = strip_prefix(&row.object_key, &aud_prefix);
                let is_root = dest == "index.html";

                sources.push(SourceDoc {
                    path,
                    markdown,
                    is_root,
                });
            }
            if sources.is_empty() {
                continue;
            }

            let opts = SiteOptions {
                audience: if audience.is_empty() {
                    None
                } else {
                    Some(audience.clone())
                },
                site_title: None,
                base_url: base_url.map(String::from),
                generate_seo: true,
                generate_feeds: true,
                style: SiteStyle::default(),
            };
            let rendered = render_site(&sources, &opts);

            let aud_opt = if audience.is_empty() {
                None
            } else {
                Some(audience.as_str())
            };

            // Write rendered pages.
            for page in &rendered.pages {
                let key = prefixed(&audience, &page.dest_filename);
                object_service
                    .put(
                        namespace_id,
                        &key,
                        "text/html; charset=utf-8",
                        page.html.as_bytes(),
                        aud_opt,
                        caller_user_id,
                    )
                    .await?;
                summary.pages_rendered += 1;
            }

            // Write static + supplementary assets.
            for (name, bytes) in &rendered.assets {
                let key = prefixed(&audience, name);
                object_service
                    .put(
                        namespace_id,
                        &key,
                        mime_for(name),
                        bytes,
                        aud_opt,
                        caller_user_id,
                    )
                    .await?;
                summary.assets_written += 1;
            }

            summary.audiences += 1;
        }

        Ok(summary)
    }

    /// Fetch a stored source's bytes via its object key.
    async fn fetch_source(
        &self,
        namespace_id: &str,
        key: &str,
        caller_user_id: &str,
    ) -> Result<Option<Vec<u8>>, ServerCoreError> {
        let object_service = ObjectService::new(
            self.namespace_store,
            self.object_meta_store,
            self.blob_store,
        );
        match object_service.get(namespace_id, key, caller_user_id).await {
            Ok(res) => Ok(Some(res.bytes)),
            Err(ServerCoreError::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// Join an audience prefix and a name, omitting the prefix for the default
/// (empty) audience.
fn prefixed(audience: &str, name: &str) -> String {
    if audience.is_empty() {
        name.to_string()
    } else {
        format!("{}/{}", audience, name)
    }
}

/// Strip a `{audience}/` prefix if present.
fn strip_prefix<'k>(key: &'k str, prefix: &str) -> &'k str {
    key.strip_prefix(prefix).unwrap_or(key)
}

/// Guess a content type from an asset filename.
fn mime_for(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if lower.ends_with(".svg") {
        "image/svg+xml"
    } else if lower.ends_with(".xml") {
        "application/xml; charset=utf-8"
    } else if lower.ends_with(".txt") {
        "text/plain; charset=utf-8"
    } else if lower.ends_with(".html") {
        "text/html; charset=utf-8"
    } else if lower.ends_with(".png") {
        "image/png"
    } else {
        "application/octet-stream"
    }
}
