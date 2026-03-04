//! PublishPlugin — WorkspacePlugin that handles HTML export and publishing.
//!
//! `PublishPlugin<FS>` is generic over the filesystem but type-erased at registration
//! via `Arc<dyn WorkspacePlugin>`. It wraps the existing `Publisher<FS>` and `Exporter<FS>`
//! to provide export functionality through the plugin command system.
//!
//! # Construction
//!
//! ```ignore
//! let plugin = PublishPlugin::new(fs.clone());
//! diaryx.plugin_registry_mut()
//!     .register_workspace_plugin(Arc::new(plugin));
//! ```

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use async_trait::async_trait;
use serde_json::Value as JsonValue;

use diaryx_core::command::{BinaryFileInfo, ExportedFile};
use diaryx_core::error::DiaryxError;
use diaryx_core::export::Exporter;
use diaryx_core::fs::AsyncFileSystem;
use diaryx_core::link_parser::LinkFormat;
use diaryx_core::plugin::{
    Plugin, PluginCapability, PluginContext, PluginError, PluginId, PluginManifest, UiContribution,
    WorkspaceOpenedEvent, WorkspacePlugin,
};

// ============================================================================
// PublishPlugin struct
// ============================================================================

/// Plugin that handles HTML export, audience filtering, and publishing.
///
/// Generic over `FS` (filesystem), but erased to `Arc<dyn WorkspacePlugin>` at registration.
pub struct PublishPlugin<FS: AsyncFileSystem + Clone> {
    fs: FS,
    workspace_root: RwLock<Option<PathBuf>>,
    link_format: RwLock<LinkFormat>,
}

// ============================================================================
// Constructors
// ============================================================================

impl<FS: AsyncFileSystem + Clone + 'static> PublishPlugin<FS> {
    /// Create a new PublishPlugin with the given filesystem.
    pub fn new(fs: FS) -> Self {
        Self {
            fs,
            workspace_root: RwLock::new(None),
            link_format: RwLock::new(LinkFormat::default()),
        }
    }
}

// ============================================================================
// Internal helpers
// ============================================================================

impl<FS: AsyncFileSystem + Clone + 'static> PublishPlugin<FS> {
    /// Resolve a workspace-relative path against the workspace root.
    fn resolve_path(&self, path: &str) -> PathBuf {
        match self.workspace_root.read().unwrap().as_ref() {
            Some(root) => root.join(path),
            None => PathBuf::from(path),
        }
    }

    /// Create an Exporter using our filesystem.
    fn exporter(&self) -> Exporter<FS> {
        Exporter::new(self.fs.clone())
    }

    /// Export files to memory as markdown, with body template rendering.
    async fn export_to_memory(
        &self,
        root_path: &Path,
        audience: &str,
    ) -> Result<Vec<ExportedFile>, DiaryxError> {
        log::debug!(
            "[PublishPlugin] ExportToMemory starting - root_path: {:?}, audience: {:?}",
            root_path,
            audience
        );

        let plan = self
            .exporter()
            .plan_export(root_path, audience, Path::new("/tmp/export"))
            .await?;

        log::debug!(
            "[PublishPlugin] plan_export returned {} included files",
            plan.included.len()
        );

        let mut files = Vec::new();
        for included in &plan.included {
            match self.fs.read_to_string(&included.source_path).await {
                Ok(content) => {
                    // When exporting for a specific audience, render body templates
                    // so {{#for-audience}} blocks resolve. For "all" (*), leave raw.
                    #[cfg(feature = "templating")]
                    let content = if audience != "*"
                        && crate::template_render::has_templates(&content)
                    {
                        match diaryx_core::frontmatter::parse_or_empty(&content) {
                            Ok(parsed) => {
                                let context = crate::template_render::build_publish_context(
                                    &parsed.frontmatter,
                                    &included.source_path,
                                    Some(root_path),
                                    audience,
                                );
                                let rendered = crate::template_render::BodyTemplateRenderer::new()
                                    .render(&parsed.body, &context)
                                    .unwrap_or_else(|_| parsed.body.clone());
                                diaryx_core::frontmatter::serialize(&parsed.frontmatter, &rendered)
                                    .unwrap_or(content)
                            }
                            Err(_) => content,
                        }
                    } else {
                        content
                    };

                    files.push(ExportedFile {
                        path: included.relative_path.to_string_lossy().to_string(),
                        content,
                    });
                }
                Err(e) => {
                    log::warn!(
                        "[PublishPlugin] read failed: {:?} - {}",
                        included.source_path,
                        e
                    );
                }
            }
        }
        log::debug!(
            "[PublishPlugin] ExportToMemory returning {} files",
            files.len()
        );
        Ok(files)
    }

    /// Export files as HTML (path extension changed, content still markdown for now).
    async fn export_to_html(
        &self,
        root_path: &Path,
        audience: &str,
    ) -> Result<Vec<ExportedFile>, DiaryxError> {
        let plan = self
            .exporter()
            .plan_export(root_path, audience, Path::new("/tmp/export"))
            .await?;

        let mut files = Vec::new();
        for included in &plan.included {
            if let Ok(content) = self.fs.read_to_string(&included.source_path).await {
                let html_path = included
                    .relative_path
                    .to_string_lossy()
                    .replace(".md", ".html");
                files.push(ExportedFile {
                    path: html_path,
                    content, // TODO: Add markdown-to-HTML conversion
                });
            }
        }
        Ok(files)
    }

    /// Collect binary attachment file paths from a workspace.
    async fn export_binary_attachments(&self, root_path: &Path) -> Vec<BinaryFileInfo> {
        let root_dir = root_path.parent().unwrap_or(root_path);

        log::info!(
            "[PublishPlugin] ExportBinaryAttachments starting - root_path: {:?}, root_dir: {:?}",
            root_path,
            root_dir
        );

        let mut attachments = Vec::new();
        let mut visited_dirs = HashSet::new();
        self.collect_binaries_recursive(root_dir, root_dir, &mut attachments, &mut visited_dirs)
            .await;

        log::info!(
            "[PublishPlugin] ExportBinaryAttachments returning {} attachment paths",
            attachments.len()
        );
        attachments
    }

    async fn collect_binaries_recursive(
        &self,
        dir: &Path,
        root_dir: &Path,
        attachments: &mut Vec<BinaryFileInfo>,
        visited_dirs: &mut HashSet<PathBuf>,
    ) {
        if visited_dirs.contains(dir) {
            return;
        }
        visited_dirs.insert(dir.to_path_buf());

        // Skip hidden directories
        if let Some(name) = dir.file_name().and_then(|n| n.to_str()) {
            if name.starts_with('.') {
                return;
            }
        }

        let entries = match self.fs.list_files(dir).await {
            Ok(e) => e,
            Err(e) => {
                log::warn!("[PublishPlugin] list_files failed for {:?}: {}", dir, e);
                return;
            }
        };

        for entry_path in entries {
            // Skip hidden files/dirs
            if let Some(name) = entry_path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with('.') {
                    continue;
                }
            }

            if self.fs.is_dir(&entry_path).await {
                Box::pin(self.collect_binaries_recursive(
                    &entry_path,
                    root_dir,
                    attachments,
                    visited_dirs,
                ))
                .await;
            } else if is_binary_file(&entry_path) {
                let relative_path = pathdiff::diff_paths(&entry_path, root_dir)
                    .unwrap_or_else(|| entry_path.clone());
                attachments.push(BinaryFileInfo {
                    source_path: entry_path.to_string_lossy().to_string(),
                    relative_path: relative_path.to_string_lossy().to_string(),
                });
            }
        }
    }
}

/// Check if a file is a binary attachment (not markdown/text).
fn is_binary_file(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase());

    match ext.as_deref() {
        // Text/markdown files - not binary
        Some("md" | "txt" | "json" | "yaml" | "yml" | "toml") => false,
        // Common binary formats
        Some(
            "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "ico" | "bmp" | "pdf" | "heic"
            | "heif" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "mp3" | "mp4" | "wav"
            | "ogg" | "flac" | "m4a" | "aac" | "mov" | "avi" | "mkv" | "webm" | "zip" | "tar"
            | "gz" | "rar" | "7z" | "ttf" | "otf" | "woff" | "woff2" | "sqlite" | "db",
        ) => true,
        _ => false,
    }
}

// ============================================================================
// Manifest
// ============================================================================

fn publish_plugin_manifest() -> PluginManifest {
    PluginManifest {
        id: PluginId("diaryx.publish".into()),
        name: "Publish".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        description: "HTML export and website publishing".into(),
        capabilities: vec![
            PluginCapability::WorkspaceEvents,
            PluginCapability::CustomCommands {
                commands: vec![
                    "ExportToHtml".into(),
                    "ExportToMemory".into(),
                    "PlanExport".into(),
                    "ExportBinaryAttachments".into(),
                    "GetExportFormats".into(),
                    "PublishWorkspace".into(),
                ],
            },
        ],
        ui: vec![UiContribution::SidebarTab {
            id: "publish-panel".into(),
            label: "Publish".into(),
            icon: None,
            side: diaryx_core::plugin::SidebarSide::Left,
            component: diaryx_core::plugin::ComponentRef::Builtin {
                component_id: "publish.panel".into(),
            },
        }],
        cli: vec![],
    }
}

// ============================================================================
// Plugin + WorkspacePlugin trait implementations
// ============================================================================

#[cfg(not(target_arch = "wasm32"))]
#[async_trait]
impl<FS: AsyncFileSystem + Clone + Send + Sync + 'static> Plugin for PublishPlugin<FS> {
    fn id(&self) -> PluginId {
        PluginId("diaryx.publish".into())
    }

    fn manifest(&self) -> PluginManifest {
        publish_plugin_manifest()
    }

    async fn init(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        if let Some(root) = &ctx.workspace_root {
            *self.workspace_root.write().unwrap() = Some(root.clone());
        }
        *self.link_format.write().unwrap() = ctx.link_format;
        Ok(())
    }
}

#[cfg(target_arch = "wasm32")]
#[async_trait(?Send)]
impl<FS: AsyncFileSystem + Clone + 'static> Plugin for PublishPlugin<FS> {
    fn id(&self) -> PluginId {
        PluginId("diaryx.publish".into())
    }

    fn manifest(&self) -> PluginManifest {
        publish_plugin_manifest()
    }

    async fn init(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        if let Some(root) = &ctx.workspace_root {
            *self.workspace_root.write().unwrap() = Some(root.clone());
        }
        *self.link_format.write().unwrap() = ctx.link_format;
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[async_trait]
impl<FS: AsyncFileSystem + Clone + Send + Sync + 'static> WorkspacePlugin for PublishPlugin<FS> {
    async fn on_workspace_opened(&self, event: &WorkspaceOpenedEvent) {
        *self.workspace_root.write().unwrap() = Some(event.workspace_root.clone());
    }

    async fn handle_command(
        &self,
        cmd: &str,
        params: JsonValue,
    ) -> Option<Result<JsonValue, PluginError>> {
        Some(self.dispatch(cmd, params).await)
    }
}

#[cfg(target_arch = "wasm32")]
#[async_trait(?Send)]
impl<FS: AsyncFileSystem + Clone + 'static> WorkspacePlugin for PublishPlugin<FS> {
    async fn on_workspace_opened(&self, event: &WorkspaceOpenedEvent) {
        *self.workspace_root.write().unwrap() = Some(event.workspace_root.clone());
    }

    async fn handle_command(
        &self,
        cmd: &str,
        params: JsonValue,
    ) -> Option<Result<JsonValue, PluginError>> {
        Some(self.dispatch(cmd, params).await)
    }
}

// ============================================================================
// String-based command dispatch (for Extism guests)
// ============================================================================

impl<FS: AsyncFileSystem + Clone + 'static> PublishPlugin<FS> {
    async fn dispatch(&self, cmd: &str, params: JsonValue) -> Result<JsonValue, PluginError> {
        match cmd {
            "PlanExport" => {
                let root_path = params["root_path"]
                    .as_str()
                    .ok_or_else(|| PluginError::CommandError("missing root_path".into()))?;
                let audience = params["audience"].as_str().unwrap_or("*");
                let resolved = self.resolve_path(root_path);
                let plan = self
                    .exporter()
                    .plan_export(&resolved, audience, Path::new("/tmp/export"))
                    .await
                    .map_err(|e| PluginError::CommandError(e.to_string()))?;
                serde_json::to_value(plan).map_err(|e| PluginError::CommandError(e.to_string()))
            }

            "ExportToMemory" => {
                let root_path = params["root_path"]
                    .as_str()
                    .ok_or_else(|| PluginError::CommandError("missing root_path".into()))?;
                let audience = params["audience"].as_str().unwrap_or("*");
                let resolved = self.resolve_path(root_path);
                let files = self
                    .export_to_memory(&resolved, audience)
                    .await
                    .map_err(|e| PluginError::CommandError(e.to_string()))?;
                serde_json::to_value(files).map_err(|e| PluginError::CommandError(e.to_string()))
            }

            "ExportToHtml" => {
                let root_path = params["root_path"]
                    .as_str()
                    .ok_or_else(|| PluginError::CommandError("missing root_path".into()))?;
                let audience = params["audience"].as_str().unwrap_or("*");
                let resolved = self.resolve_path(root_path);
                let files = self
                    .export_to_html(&resolved, audience)
                    .await
                    .map_err(|e| PluginError::CommandError(e.to_string()))?;
                serde_json::to_value(files).map_err(|e| PluginError::CommandError(e.to_string()))
            }

            "ExportBinaryAttachments" => {
                let root_path = params["root_path"]
                    .as_str()
                    .ok_or_else(|| PluginError::CommandError("missing root_path".into()))?;
                let resolved = self.resolve_path(root_path);
                let attachments = self.export_binary_attachments(&resolved).await;
                serde_json::to_value(attachments)
                    .map_err(|e| PluginError::CommandError(e.to_string()))
            }

            "GetExportFormats" => {
                let formats = serde_json::json!([
                    { "id": "markdown", "label": "Markdown", "extension": ".md", "binary": false, "requiresConverter": false },
                    { "id": "html", "label": "HTML", "extension": ".html", "binary": false, "requiresConverter": false },
                    { "id": "pdf", "label": "PDF", "extension": ".pdf", "binary": true, "requiresConverter": true },
                    { "id": "docx", "label": "Word (DOCX)", "extension": ".docx", "binary": true, "requiresConverter": true },
                    { "id": "epub", "label": "EPUB", "extension": ".epub", "binary": true, "requiresConverter": true },
                    { "id": "latex", "label": "LaTeX", "extension": ".tex", "binary": false, "requiresConverter": true },
                    { "id": "odt", "label": "OpenDocument (ODT)", "extension": ".odt", "binary": true, "requiresConverter": true },
                    { "id": "rst", "label": "reStructuredText", "extension": ".rst", "binary": false, "requiresConverter": true },
                ]);
                Ok(formats)
            }

            #[cfg(not(target_arch = "wasm32"))]
            "PublishWorkspace" => {
                let workspace_root = params["workspace_root"]
                    .as_str()
                    .ok_or_else(|| PluginError::CommandError("missing workspace_root".into()))?;
                let destination = params["destination"]
                    .as_str()
                    .ok_or_else(|| PluginError::CommandError("missing destination".into()))?;

                let resolved_root = self.resolve_path(workspace_root);
                let dest_path = PathBuf::from(destination);

                let options = crate::types::PublishOptions {
                    single_file: params["single_file"].as_bool().unwrap_or(false),
                    title: params["title"].as_str().map(String::from),
                    audience: params["audience"].as_str().map(String::from),
                    force: params["force"].as_bool().unwrap_or(false),
                    copy_attachments: params["copy_attachments"].as_bool().unwrap_or(true),
                };

                let publisher = crate::publisher::Publisher::new(self.fs.clone());
                let result = publisher
                    .publish(&resolved_root, &dest_path, &options)
                    .await
                    .map_err(|e| PluginError::CommandError(e.to_string()))?;

                Ok(serde_json::json!({
                    "files_processed": result.files_processed,
                    "attachments_copied": result.attachments_copied,
                }))
            }

            _ => Err(PluginError::CommandError(format!(
                "Unknown publish command: {}",
                cmd
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use diaryx_core::fs::{InMemoryFileSystem, SyncToAsyncFs};

    type TestFs = SyncToAsyncFs<InMemoryFileSystem>;

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        futures_lite::future::block_on(f)
    }

    fn create_test_plugin() -> PublishPlugin<TestFs> {
        let fs = SyncToAsyncFs::new(InMemoryFileSystem::new());
        PublishPlugin::new(fs)
    }

    #[test]
    fn test_manifest() {
        let plugin = create_test_plugin();
        let manifest = plugin.manifest();
        assert_eq!(manifest.id.0, "publish");
        assert_eq!(manifest.name, "Publish");
        assert!(!manifest.ui.is_empty());
    }

    #[test]
    fn test_get_export_formats() {
        let plugin = create_test_plugin();
        let result = block_on(plugin.dispatch("GetExportFormats", serde_json::json!({})));
        assert!(result.is_ok());
        let formats = result.unwrap();
        assert!(formats.is_array());
        let arr = formats.as_array().unwrap();
        assert_eq!(arr.len(), 8);
        assert_eq!(arr[0]["id"], "markdown");
        assert_eq!(arr[1]["id"], "html");
        assert_eq!(arr[2]["id"], "pdf");
    }
}
