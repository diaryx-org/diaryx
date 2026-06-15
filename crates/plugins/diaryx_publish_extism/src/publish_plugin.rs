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

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use serde_json::Value as JsonValue;

use crate::publish::body_renderer::BodyRenderer;
use crate::publish::publish_format::PublishFormat;
use crate::publish::publisher::prepare_published_attachment_bytes;
use diaryx_core::error::DiaryxError;
use diaryx_core::fs::AsyncFileSystem;
use diaryx_core::link_parser::LinkFormat;
use diaryx_core::plugin::{
    Plugin, PluginCapability, PluginContext, PluginError, PluginId, PluginManifest, UiContribution,
    WorkspaceOpenedEvent, WorkspacePlugin,
};
use diaryx_core::workspace::Workspace;

// ============================================================================
// PublishPlugin struct
// ============================================================================

/// Per-audience access control state.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum AudienceAccessState {
    Unpublished,
    Public,
    AccessControl,
}

impl Default for AudienceAccessState {
    fn default() -> Self {
        Self::Unpublished
    }
}

/// Per-audience publish configuration.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct AudiencePublishConfig {
    pub state: AudienceAccessState,
    /// Access control method when state is `AccessControl` (e.g. "access-key").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_method: Option<String>,
}

/// Configuration for the publish plugin, stored in root frontmatter at
/// `plugins.diaryx.publish`.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct PublishPluginConfig {
    /// Which audience tags are freely accessible (no token required).
    /// Derived from `audience_states` for backward compatibility.
    #[serde(default)]
    pub public_audiences: Vec<String>,
    /// Per-audience publish state and access control settings.
    #[serde(default)]
    pub audience_states: std::collections::HashMap<String, AudiencePublishConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subdomain: Option<String>,
    /// Server's site base URL for direct serving (e.g. "http://localhost:3030").
    /// Written by the UI when it fetches server capabilities.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site_base_url: Option<String>,
    /// Domain for subdomain-based routing (e.g. "diaryx.org").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site_domain: Option<String>,
}

/// Plugin that handles HTML export, audience filtering, and publishing.
///
/// Generic over `FS` (filesystem), but erased to `Arc<dyn WorkspacePlugin>` at registration.
pub struct PublishPlugin<FS: AsyncFileSystem + Clone> {
    fs: FS,
    workspace_root: RwLock<Option<PathBuf>>,
    link_format: RwLock<LinkFormat>,
    config: RwLock<PublishPluginConfig>,
    body_renderer: Arc<dyn BodyRenderer>,
    format: Arc<dyn PublishFormat>,
}

// ============================================================================
// Constructors
// ============================================================================

impl<FS: AsyncFileSystem + Clone + 'static> PublishPlugin<FS> {
    /// Create a new PublishPlugin with the given filesystem, body renderer, and format.
    pub fn with_renderer_and_format(
        fs: FS,
        body_renderer: Arc<dyn BodyRenderer>,
        format: Arc<dyn PublishFormat>,
    ) -> Self {
        Self {
            fs,
            workspace_root: RwLock::new(None),
            link_format: RwLock::new(LinkFormat::default()),
            config: RwLock::new(PublishPluginConfig::default()),
            body_renderer,
            format,
        }
    }

    /// Create a new PublishPlugin with the given filesystem and body renderer,
    /// using the default HTML format.
    pub fn with_renderer(fs: FS, body_renderer: Arc<dyn BodyRenderer>) -> Self {
        Self::with_renderer_and_format(
            fs,
            body_renderer,
            Arc::new(crate::publish::HtmlFormat::new()),
        )
    }

    /// Create a new PublishPlugin with the given filesystem, using the default
    /// HTML format and a noop body renderer.
    pub fn new(fs: FS) -> Self {
        Self::with_renderer(fs, Arc::new(crate::publish::NoopBodyRenderer))
    }
}

// ============================================================================
// Internal helpers
// ============================================================================

impl<FS: AsyncFileSystem + Clone + 'static> PublishPlugin<FS> {
    fn workspace_dir_from_path(path: &Path) -> PathBuf {
        if matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some(ext) if ext.eq_ignore_ascii_case("md")
        ) {
            path.parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| path.to_path_buf())
        } else {
            path.to_path_buf()
        }
    }

    async fn resolve_root_index_path(&self, workspace_path: &Path) -> Result<PathBuf, DiaryxError> {
        if matches!(
            workspace_path.extension().and_then(|ext| ext.to_str()),
            Some(ext) if ext.eq_ignore_ascii_case("md")
        ) {
            return Ok(workspace_path.to_path_buf());
        }

        let workspace = Workspace::new(self.fs.clone());
        workspace
            .find_root_index_in_dir(workspace_path)
            .await?
            .ok_or_else(|| {
                DiaryxError::Unsupported(format!(
                    "no root index found in workspace '{}'",
                    workspace_path.display()
                ))
            })
    }

    async fn current_root_index_path(&self) -> Result<PathBuf, DiaryxError> {
        let workspace_path = self
            .workspace_root
            .read()
            .unwrap()
            .clone()
            .ok_or_else(|| DiaryxError::Unsupported("no workspace root".into()))?;
        self.resolve_root_index_path(&workspace_path).await
    }

    fn current_workspace_dir(&self) -> Option<PathBuf> {
        self.workspace_root
            .read()
            .unwrap()
            .clone()
            .map(|path| Self::workspace_dir_from_path(&path))
    }

    /// Resolve a workspace-relative path against the workspace root.
    #[allow(dead_code)]
    fn resolve_path(&self, path: &str) -> PathBuf {
        match self.current_workspace_dir() {
            Some(root) => root.join(path),
            None => PathBuf::from(path),
        }
    }

    /// Load publish plugin config from root frontmatter `plugins.diaryx.publish`.
    async fn load_config(&self) {
        let root = match self.current_root_index_path().await {
            Ok(root) => root,
            Err(_) => return,
        };
        if let Ok(content) = self.fs.read_to_string(&root).await {
            if let Ok(parsed) = diaryx_core::frontmatter::parse_or_empty(&content) {
                let config = parsed
                    .frontmatter
                    .get("plugins")
                    .and_then(|v| v.get("diaryx.publish"))
                    .and_then(|v| {
                        // Convert yaml::Value to JSON then deserialize
                        serde_json::to_value(v)
                            .ok()
                            .and_then(|jv| serde_json::from_value::<PublishPluginConfig>(jv).ok())
                    })
                    .unwrap_or_default();
                *self.config.write().unwrap() = config;
            }
        }
    }

    /// Save publish plugin config to root frontmatter `plugins.diaryx.publish`.
    async fn save_config_to_frontmatter(&self) -> Result<(), DiaryxError> {
        let root = self.current_root_index_path().await?;
        let content = self
            .fs
            .read_to_string(&root)
            .await
            .map_err(|e| DiaryxError::FileRead {
                path: root.clone(),
                source: e,
            })?;
        let parsed = diaryx_core::frontmatter::parse_or_empty(&content)?;
        let mut fm = parsed.frontmatter.clone();

        let config = self.config.read().unwrap().clone();
        let config_yaml_str = diaryx_core::yaml::to_string(&config).map_err(DiaryxError::Yaml)?;
        let config_yaml: diaryx_core::yaml::Value =
            diaryx_core::yaml::from_str(&config_yaml_str).map_err(DiaryxError::Yaml)?;

        // Store config under `plugins."diaryx.publish"` (dotted key, matching
        // the canonical plugin ID used by the permissions system).
        let plugins_key = "plugins".to_string();
        let plugins_val = fm
            .entry(plugins_key)
            .or_insert_with(|| diaryx_core::yaml::Value::Mapping(indexmap::IndexMap::new()));
        if let Some(plugins_map) = plugins_val.as_mapping_mut() {
            // Merge into existing "diaryx.publish" entry (preserves permissions).
            let entry = plugins_map
                .entry("diaryx.publish".into())
                .or_insert_with(|| diaryx_core::yaml::Value::Mapping(indexmap::IndexMap::new()));
            if let (Some(existing), Some(config_map)) =
                (entry.as_mapping_mut(), config_yaml.as_mapping())
            {
                for (k, v) in config_map {
                    existing.insert(k.clone(), v.clone());
                }
            }
        }

        let new_content = diaryx_core::frontmatter::serialize(&fm, &parsed.body)?;
        self.fs.write(&root, new_content.as_bytes()).await?;
        Ok(())
    }

    /// Read default_audience from workspace config.
    async fn default_audience(&self) -> Option<String> {
        let root = self.current_root_index_path().await.ok()?;
        let ws = Workspace::new(self.fs.clone());
        ws.get_workspace_config(&root)
            .await
            .ok()
            .and_then(|c| c.default_audience)
    }

    /// Read the workspace-level audience declaration + migration flag from
    /// the root index frontmatter. Returns `(audiences, migrated)`. Either
    /// or both may be `None`.
    async fn read_workspace_audiences(
        &self,
    ) -> (Option<Vec<diaryx_core::workspace::AudienceDecl>>, bool) {
        let Ok(root) = self.current_root_index_path().await else {
            return (None, false);
        };
        let ws = Workspace::new(self.fs.clone());
        match ws.get_workspace_config(&root).await {
            Ok(c) => (c.audiences, c.audiences_migrated.unwrap_or(false)),
            Err(_) => (None, false),
        }
    }

    /// Load the workspace's theme and return an `HtmlFormat` configured with it.
    ///
    /// Reads workspace appearance files and returns an `HtmlFormat` with the
    /// resolved theme. Falls back to the default format if no settings exist.
    async fn format_with_workspace_theme(&self) -> Arc<dyn PublishFormat> {
        let workspace_dir = match self.current_workspace_dir() {
            Some(root) => root,
            None => return self.format.clone(),
        };

        let theme =
            match diaryx_core::appearance::resolve_appearance(&self.fs, &workspace_dir).await {
                Some(t) => t,
                None => return self.format.clone(),
            };

        Arc::new(crate::publish::HtmlFormat::with_theme(theme))
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
                    "PublishWorkspace".into(),
                    "PublishToNamespace".into(),
                    "PreviewPublish".into(),
                    "GetPublishConfig".into(),
                    "SetPublishConfig".into(),
                    "GetAudiencePublishStates".into(),
                    "SetAudiencePublishState".into(),
                ],
            },
        ],
        ui: vec![UiContribution::SidebarTab {
            id: "publish-panel".into(),
            label: "Publish".into(),
            icon: Some("globe".into()),
            side: diaryx_core::plugin::SidebarSide::Left,
            component: diaryx_core::plugin::ComponentRef::Declarative {
                fields: vec![
                    diaryx_core::plugin::SettingsField::HostWidget {
                        widget_id: "namespace.guard".into(),
                        sign_in_action: Some(diaryx_core::plugin::HostAction {
                            action_type: "open-settings".into(),
                            payload: Some(serde_json::json!({ "tab": "account" })),
                        }),
                    },
                    diaryx_core::plugin::SettingsField::HostWidget {
                        widget_id: "namespace.site-url".into(),
                        sign_in_action: None,
                    },
                    diaryx_core::plugin::SettingsField::HostWidget {
                        widget_id: "namespace.subdomain".into(),
                        sign_in_action: None,
                    },
                    diaryx_core::plugin::SettingsField::HostWidget {
                        widget_id: "namespace.custom-domains".into(),
                        sign_in_action: None,
                    },
                    diaryx_core::plugin::SettingsField::HostWidget {
                        widget_id: "namespace.audiences".into(),
                        sign_in_action: None,
                    },
                    diaryx_core::plugin::SettingsField::HostWidget {
                        widget_id: "namespace.publish-button".into(),
                        sign_in_action: None,
                    },
                ],
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
        self.load_config().await;
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
        self.load_config().await;
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
    /// Render every resolved audience and diff it against what the namespace
    /// already holds, so the apply step only uploads changed content.
    ///
    /// Lists existing objects and server audiences exactly once. Performs no
    /// mutations, so it backs both `PreviewPublish` and `PublishToNamespace`.
    async fn compute_publish_plan(
        &self,
        namespace_id: &str,
    ) -> Result<(crate::publish::plan::PublishPlan, AudienceSource), PluginError> {
        use crate::publish::plan::{self, AudiencePlan};
        use std::collections::HashMap;

        let workspace_root = self
            .current_root_index_path()
            .await
            .map_err(|e| PluginError::CommandError(e.to_string()))?;

        let config = self.config.read().unwrap().clone();
        let default_aud = self.default_audience().await;
        let format = self.format_with_workspace_theme().await;

        // Dual-read: prefer workspace-file `audiences:` declaration; fall back
        // to the legacy `audience_states` HashMap when absent.
        let (workspace_audiences, audiences_migrated) = self.read_workspace_audiences().await;
        let (resolved, audience_source) =
            resolve_audiences(workspace_audiences.as_deref(), &config);

        // List existing objects ONCE; index by audience → (key → content_hash).
        let mut existing_by_audience: HashMap<String, HashMap<String, Option<String>>> =
            HashMap::new();
        let objects =
            diaryx_plugin_sdk::host::namespace::list_objects(namespace_id).map_err(|e| {
                PluginError::CommandError(format!("failed to list namespace objects: {}", e))
            })?;
        for obj in objects {
            if let Some(aud) = obj.audience {
                existing_by_audience
                    .entry(aud)
                    .or_default()
                    .insert(obj.key, obj.content_hash);
            }
        }

        let mut audience_plans: Vec<AudiencePlan> = Vec::new();

        for audience in &resolved {
            let audience_name = &audience.name;
            let existing = existing_by_audience
                .get(audience_name)
                .cloned()
                .unwrap_or_default();

            // Legacy `Unpublished`: render nothing, delete every object it has.
            if !audience.publish {
                let mut deletes: Vec<String> = existing.keys().cloned().collect();
                deletes.sort();
                audience_plans.push(AudiencePlan {
                    name: audience_name.clone(),
                    gates: audience.gates.clone(),
                    uploads: Vec::new(),
                    unchanged: 0,
                    deletes,
                    publish: false,
                    stale: false,
                });
                continue;
            }

            // Render for this audience.
            let options = crate::publish::PublishOptions {
                audience: Some(audience_name.clone()),
                default_audience: default_aud.clone(),
                ..Default::default()
            };
            let publisher =
                crate::publish::Publisher::new(self.fs.clone(), &*self.body_renderer, &*format);
            let (rendered, attachment_paths) = publisher
                .render_with_attachments(&workspace_root, &options)
                .await
                .map_err(|e| PluginError::CommandError(e.to_string()))?;

            // No entries carry this audience tag → stale. Leave its objects
            // untouched (matching prior behavior); the caller prunes it from
            // legacy config / strict-sync.
            if rendered.is_empty() {
                audience_plans.push(AudiencePlan {
                    name: audience_name.clone(),
                    gates: audience.gates.clone(),
                    uploads: Vec::new(),
                    unchanged: 0,
                    deletes: Vec::new(),
                    publish: true,
                    stale: true,
                });
                continue;
            }

            // Assemble the planned object set: rendered pages + attachments.
            // Track which object key carries which source-file ARK so we can
            // tag the resulting uploads after the diff (publish = registration).
            let mut planned: Vec<(String, Vec<u8>, String)> =
                Vec::with_capacity(rendered.len() + attachment_paths.len());
            let mut key_to_ark: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            for file in rendered {
                let key = format!("{}/{}", audience_name, file.path);
                if let Some(ark) = &file.file_ark {
                    key_to_ark.insert(key.clone(), ark.clone());
                }
                planned.push((key, file.content, file.mime_type));
            }
            for (src_path, dest_rel) in &attachment_paths {
                if !self.fs.try_exists(src_path).await.unwrap_or(false) {
                    log::warn!(
                        "Skipping attachment {}: source file does not exist",
                        src_path.display()
                    );
                    continue;
                }
                match self.fs.read(src_path).await {
                    Ok(bytes) => {
                        let mime = mime_type_from_ext(dest_rel);
                        let prepared = prepare_published_attachment_bytes(dest_rel, &bytes);
                        let key = format!("{}/{}", audience_name, dest_rel.display());
                        planned.push((key, prepared, mime));
                    }
                    Err(e) => {
                        log::warn!("Skipping attachment {}: {}", src_path.display(), e);
                    }
                }
            }

            let mut audience_plan = plan::diff_audience(
                audience_name.clone(),
                audience.gates.clone(),
                planned,
                &existing,
            );
            // Tag content-page uploads with their source-file ARK so the server
            // registers `(workspace_ark, file_ark) -> key` on upload.
            for up in &mut audience_plan.uploads {
                up.file_ark = key_to_ark.get(&up.key).cloned();
            }
            audience_plans.push(audience_plan);
        }

        // Strict-sync: when the file is canonical, any server audience not
        // declared in the file is slated for deletion. Single list call.
        let mut audiences_to_delete: Vec<String> = Vec::new();
        if audience_source == AudienceSource::File && audiences_migrated {
            match diaryx_plugin_sdk::host::namespace::list_audiences(namespace_id) {
                Ok(server_audiences) => {
                    let declared: std::collections::HashSet<&str> =
                        resolved.iter().map(|a| a.name.as_str()).collect();
                    for server_aud in server_audiences {
                        if !declared.contains(server_aud.as_str()) {
                            audiences_to_delete.push(server_aud);
                        }
                    }
                }
                Err(e) => {
                    log::warn!(
                        "Strict-sync: failed to list server audiences for cleanup: {}",
                        e
                    );
                }
            }
        }

        let plan = plan::PublishPlan::new(audience_plans, audiences_to_delete, audiences_migrated);
        Ok((plan, audience_source))
    }

    async fn dispatch(&self, cmd: &str, params: JsonValue) -> Result<JsonValue, PluginError> {
        match cmd {
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

                let default_aud = self.default_audience().await;
                let options = crate::publish::PublishOptions {
                    single_file: params["single_file"].as_bool().unwrap_or(false),
                    title: params["title"].as_str().map(String::from),
                    audience: params["audience"].as_str().map(String::from),
                    force: params["force"].as_bool().unwrap_or(false),
                    copy_attachments: params["copy_attachments"].as_bool().unwrap_or(true),
                    default_audience: default_aud,
                    ..Default::default()
                };

                let format = self.format_with_workspace_theme().await;
                let publisher =
                    crate::publish::Publisher::new(self.fs.clone(), &*self.body_renderer, &*format);
                let result = publisher
                    .publish(&resolved_root, &dest_path, &options)
                    .await
                    .map_err(|e| PluginError::CommandError(e.to_string()))?;

                Ok(serde_json::json!({
                    "files_processed": result.files_processed,
                    "attachments_copied": result.attachments_copied,
                }))
            }

            "GetPublishConfig" => {
                let config = self.config.read().unwrap().clone();
                serde_json::to_value(config).map_err(|e| PluginError::CommandError(e.to_string()))
            }

            "SetPublishConfig" => {
                let new_config: PublishPluginConfig = serde_json::from_value(params)
                    .map_err(|e| PluginError::CommandError(format!("invalid config: {}", e)))?;
                *self.config.write().unwrap() = new_config;
                self.save_config_to_frontmatter()
                    .await
                    .map_err(|e| PluginError::CommandError(e.to_string()))?;
                Ok(serde_json::json!({ "ok": true }))
            }

            "GetAudiencePublishStates" => {
                let config = self.config.read().unwrap().clone();
                serde_json::to_value(&config.audience_states)
                    .map_err(|e| PluginError::CommandError(e.to_string()))
            }

            "SetAudiencePublishState" => {
                let audience = params["audience"]
                    .as_str()
                    .ok_or_else(|| PluginError::CommandError("missing audience".into()))?
                    .to_string();
                let state_config: AudiencePublishConfig =
                    serde_json::from_value(params["config"].clone())
                        .map_err(|e| PluginError::CommandError(format!("invalid config: {}", e)))?;

                // Sync audience gates to the server if namespace is configured.
                // Unpublished audiences are left alone here — the server-side
                // record is only removed if the writer explicitly deletes it
                // (or, in a later pass, through strict file-as-truth sync).
                let namespace_id = {
                    let config = self.config.read().unwrap();
                    config.namespace_id.clone()
                };
                if let Some(ns_id) = &namespace_id {
                    if state_config.state != AudienceAccessState::Unpublished {
                        let gates = gates_for_state(&state_config);
                        // Best-effort: don't fail the whole command if server sync fails.
                        if let Err(e) = diaryx_plugin_sdk::host::namespace::sync_audience(
                            ns_id, &audience, &gates,
                        ) {
                            log::warn!("Failed to sync audience '{}' to server: {}", audience, e);
                        }
                    }
                }

                {
                    let mut config = self.config.write().unwrap();
                    if state_config.state == AudienceAccessState::Unpublished {
                        config.audience_states.remove(&audience);
                        config.public_audiences.retain(|a| a != &audience);
                    } else {
                        if state_config.state == AudienceAccessState::Public {
                            if !config.public_audiences.contains(&audience) {
                                config.public_audiences.push(audience.clone());
                            }
                        } else {
                            config.public_audiences.retain(|a| a != &audience);
                        }
                        config
                            .audience_states
                            .insert(audience.clone(), state_config);
                    }
                }

                self.save_config_to_frontmatter()
                    .await
                    .map_err(|e| PluginError::CommandError(e.to_string()))?;
                let config = self.config.read().unwrap().clone();
                serde_json::to_value(&config.audience_states)
                    .map_err(|e| PluginError::CommandError(e.to_string()))
            }

            "PreviewPublish" => {
                let namespace_id = params["namespace_id"]
                    .as_str()
                    .ok_or_else(|| PluginError::CommandError("missing namespace_id".into()))?
                    .to_string();

                let (plan, audience_source) = self.compute_publish_plan(&namespace_id).await?;
                let mut summary = plan.to_summary_json();
                if let Some(obj) = summary.as_object_mut() {
                    obj.insert(
                        "audience_source".into(),
                        serde_json::json!(match audience_source {
                            AudienceSource::File => "file",
                            AudienceSource::LegacyPluginConfig => "legacy_plugin_config",
                        }),
                    );
                }
                Ok(summary)
            }

            "PublishToNamespace" => {
                let namespace_id = params["namespace_id"]
                    .as_str()
                    .ok_or_else(|| PluginError::CommandError("missing namespace_id".into()))?
                    .to_string();

                // Compute the diff once (lists existing objects + server
                // audiences a single time, and skips unchanged content).
                let (plan, audience_source) = self.compute_publish_plan(&namespace_id).await?;
                let total_uploads = plan.totals.uploads;

                emit_publish_progress(serde_json::json!({
                    "type": "PublishProgress",
                    "phase": "start",
                    "current": 0,
                    "total": total_uploads,
                }));

                // --- Phase 1: sync gate stacks for audiences with content. ---
                for ap in &plan.audiences {
                    if ap.publish && !ap.stale {
                        if let Err(e) = diaryx_plugin_sdk::host::namespace::sync_audience(
                            &namespace_id,
                            &ap.name,
                            &ap.gates,
                        ) {
                            return Err(PluginError::CommandError(format!(
                                "failed to sync audience {}: {}",
                                ap.name, e
                            )));
                        }
                    }
                }

                // --- Phase 2: upload everything first. On the first error we
                // return immediately, having run NO deletes — so a failed
                // publish never removes content from the live site (worst case
                // is an additive, partially-updated site). ---
                let mut uploaded = 0usize;
                let mut bytes_uploaded: u64 = 0;
                for ap in &plan.audiences {
                    for up in &ap.uploads {
                        diaryx_plugin_sdk::host::namespace::put_object_with_ark(
                            &namespace_id,
                            &up.key,
                            &up.bytes,
                            &up.mime_type,
                            Some(ap.name.as_str()),
                            up.file_ark.as_deref(),
                        )
                        .map_err(|e| {
                            PluginError::CommandError(format!("failed to upload {}: {}", up.key, e))
                        })?;
                        uploaded += 1;
                        bytes_uploaded += up.bytes.len() as u64;
                        emit_publish_progress(serde_json::json!({
                            "type": "PublishProgress",
                            "phase": "uploading",
                            "audience": ap.name,
                            "current": uploaded,
                            "total": total_uploads,
                        }));
                    }
                }

                // --- Phase 3: deletes. Only reached when every upload
                // succeeded. Stale-object deletes come from the planned key set
                // (computed in the plan), so a file is never removed merely
                // because its upload was skipped as unchanged. ---
                emit_publish_progress(serde_json::json!({
                    "type": "PublishProgress",
                    "phase": "finalizing",
                    "current": uploaded,
                    "total": total_uploads,
                }));

                let mut deleted = 0usize;
                for ap in &plan.audiences {
                    for key in &ap.deletes {
                        let _ =
                            diaryx_plugin_sdk::host::namespace::delete_object(&namespace_id, key);
                        deleted += 1;
                    }
                }

                // Strict-sync audience deletion (also revokes their tokens).
                let mut audiences_deleted: Vec<String> = Vec::new();
                for name in &plan.audiences_to_delete {
                    match diaryx_plugin_sdk::host::namespace::delete_audience(&namespace_id, name) {
                        Ok(()) => audiences_deleted.push(name.clone()),
                        Err(e) => {
                            log::warn!("Strict-sync: failed to delete audience '{}': {}", name, e)
                        }
                    }
                }

                // Legacy plugin-config cleanup: drop audiences that rendered no
                // entries this run (legacy-source path only).
                let stale_audiences: Vec<String> = plan
                    .audiences
                    .iter()
                    .filter(|a| a.stale)
                    .map(|a| a.name.clone())
                    .collect();
                if !stale_audiences.is_empty()
                    && audience_source == AudienceSource::LegacyPluginConfig
                {
                    {
                        let mut config = self.config.write().unwrap();
                        for name in &stale_audiences {
                            config.audience_states.remove(name);
                            config.public_audiences.retain(|a| a != name);
                        }
                    }
                    if let Err(e) = self.save_config_to_frontmatter().await {
                        log::warn!("Failed to persist stale audience cleanup: {}", e);
                    }
                }

                let audiences_published: Vec<String> = plan
                    .audiences
                    .iter()
                    .filter(|a| a.publish && !a.stale)
                    .map(|a| a.name.clone())
                    .collect();

                emit_publish_progress(serde_json::json!({
                    "type": "PublishProgress",
                    "phase": "done",
                    "current": uploaded,
                    "total": total_uploads,
                }));

                Ok(serde_json::json!({
                    // Back-compat keys.
                    "audiences_published": audiences_published,
                    "audiences_deleted": audiences_deleted,
                    "audience_source": match audience_source {
                        AudienceSource::File => "file",
                        AudienceSource::LegacyPluginConfig => "legacy_plugin_config",
                    },
                    "audiences_migrated": plan.audiences_migrated,
                    "files_uploaded": uploaded,
                    "files_deleted": deleted,
                    // Richer receipt.
                    "uploaded": uploaded,
                    "skipped_unchanged": plan.totals.unchanged,
                    "deleted": deleted,
                    "bytes_uploaded": bytes_uploaded,
                }))
            }

            _ => Err(PluginError::CommandError(format!(
                "Unknown publish command: {}",
                cmd
            ))),
        }
    }
}

/// Map a legacy `AudiencePublishConfig` to the gate stack the server expects.
///
/// - `Public` → `[]` (no gates; anyone with the URL reads).
/// - `AccessControl` → `[{"kind":"link"}]` (magic-link token required).
///   Future: an `access_method: "password"` could produce an additional
///   `{"kind":"password"}` gate here.
/// - `Unpublished` is expected to short-circuit before reaching this helper.
fn gates_for_state(config: &AudiencePublishConfig) -> serde_json::Value {
    match config.state {
        AudienceAccessState::Public => serde_json::json!([]),
        AudienceAccessState::AccessControl => serde_json::json!([{ "kind": "link" }]),
        AudienceAccessState::Unpublished => serde_json::json!([]),
    }
}

/// Map a workspace-file `Gate` enum value to the JSON shape the server's
/// `sync_audience` endpoint expects.
fn gate_to_json(gate: &diaryx_core::workspace::Gate) -> serde_json::Value {
    match gate {
        diaryx_core::workspace::Gate::Link => serde_json::json!({ "kind": "link" }),
        diaryx_core::workspace::Gate::Password => serde_json::json!({ "kind": "password" }),
    }
}

/// One resolved audience the publish flow will operate on, regardless of
/// whether the audience came from the workspace file (`audiences:`) or the
/// legacy plugin-config HashMap (`audience_states`).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedAudience {
    pub name: String,
    /// Gates JSON ready to send to the server's `sync_audience` endpoint.
    pub gates: serde_json::Value,
    /// True when this audience is "publishable" — has at least one entry
    /// to render. Legacy `Unpublished` audiences from `audience_states`
    /// produce `false` here so the caller knows to skip them; file-declared
    /// audiences always produce `true` (the file is opt-in by definition).
    pub publish: bool,
}

/// Audience source used for a given resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudienceSource {
    /// Sourced from `WorkspaceConfig.audiences` (the new file-as-truth path).
    File,
    /// Sourced from `PublishPluginConfig.audience_states` (the legacy path).
    LegacyPluginConfig,
}

/// Resolve the active audience set, preferring the workspace-file
/// declaration over the legacy plugin-config HashMap.
///
/// Returns the list of resolved audiences plus the source that was used —
/// callers gate strict-sync deletion on `source == File && migrated`.
pub fn resolve_audiences(
    workspace_audiences: Option<&[diaryx_core::workspace::AudienceDecl]>,
    plugin_config: &PublishPluginConfig,
) -> (Vec<ResolvedAudience>, AudienceSource) {
    if let Some(decls) = workspace_audiences {
        let resolved = decls
            .iter()
            .map(|d| ResolvedAudience {
                name: d.name.clone(),
                gates: serde_json::Value::Array(d.gates.iter().map(gate_to_json).collect()),
                publish: true,
            })
            .collect();
        return (resolved, AudienceSource::File);
    }

    // Legacy fallback — preserve existing publishability semantics: only
    // audiences whose state isn't `Unpublished` are publishable.
    let resolved = plugin_config
        .audience_states
        .iter()
        .map(|(name, cfg)| ResolvedAudience {
            name: name.clone(),
            gates: gates_for_state(cfg),
            publish: cfg.state != AudienceAccessState::Unpublished,
        })
        .collect();
    (resolved, AudienceSource::LegacyPluginConfig)
}

/// Best-effort publish progress emission.
///
/// Events whose `type` starts with an uppercase letter are forwarded to the
/// frontend over the existing `extism-filesystem-event` channel on both the
/// Tauri and browser backends (see `TauriEventEmitter` / `emitPluginHostEvent`),
/// where the publish panel subscribes via `backend.onFileSystemEvent`. Emission
/// failures are non-fatal — progress is advisory.
fn emit_publish_progress(event: serde_json::Value) {
    if let Ok(json) = serde_json::to_string(&event) {
        let _ = diaryx_plugin_sdk::host::events::emit(&json);
    }
}

/// Infer MIME type from a file extension. Falls back to `application/octet-stream`.
fn mime_type_from_ext(path: &Path) -> String {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("html" | "htm") => "text/html",
        Some("svg") => "image/svg+xml",
        Some("webp") => "image/webp",
        Some("pdf") => "application/pdf",
        Some("ico") => "image/x-icon",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("mp3") => "audio/mpeg",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        _ => "application/octet-stream",
    }
    .to_string()
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
        assert_eq!(manifest.id.0, "diaryx.publish");
        assert_eq!(manifest.name, "Publish");
        assert!(!manifest.ui.is_empty());

        let diaryx_core::plugin::UiContribution::SidebarTab {
            component, icon, ..
        } = &manifest.ui[0]
        else {
            panic!("expected publish sidebar tab contribution");
        };
        assert_eq!(icon.as_deref(), Some("globe"));
        match component {
            diaryx_core::plugin::ComponentRef::Declarative { fields } => {
                assert!(matches!(
                    &fields[0],
                    diaryx_core::plugin::SettingsField::HostWidget { widget_id, .. }
                        if widget_id == "namespace.guard"
                ));
            }
            other => panic!("expected declarative component, got {other:?}"),
        }
    }

    #[test]
    fn saves_config_to_root_index_when_workspace_root_is_directory() {
        let plugin = create_test_plugin();
        let root_index = PathBuf::from("workspace/Diaryx.md");
        let workspace_dir = PathBuf::from("workspace");

        block_on(plugin.fs.write(&root_index, b"---\ncontents: []\n---\n"))
            .expect("root index write should succeed");
        *plugin.workspace_root.write().unwrap() = Some(workspace_dir);
        plugin.config.write().unwrap().namespace_id = Some("ns_123".into());

        block_on(plugin.save_config_to_frontmatter()).expect("config save should succeed");

        let content = block_on(plugin.fs.read_to_string(&root_index)).expect("root index exists");
        assert!(content.contains("namespace_id: ns_123"));
    }

    #[test]
    fn resolve_path_uses_workspace_directory_when_workspace_root_is_file() {
        let plugin = create_test_plugin();
        *plugin.workspace_root.write().unwrap() = Some(PathBuf::from("workspace/Diaryx.md"));

        assert_eq!(
            plugin.resolve_path("assets/cover.md"),
            PathBuf::from("workspace/assets/cover.md")
        );
    }

    #[test]
    fn current_root_index_path_resolves_workspace_directory() {
        let plugin = create_test_plugin();
        let root_index = PathBuf::from("workspace/Diaryx.md");

        block_on(plugin.fs.write(&root_index, b"---\ncontents: []\n---\n"))
            .expect("root index write should succeed");
        *plugin.workspace_root.write().unwrap() = Some(PathBuf::from("workspace"));

        let resolved =
            block_on(plugin.current_root_index_path()).expect("root index should resolve");
        assert_eq!(resolved, root_index);
    }

    #[test]
    fn mime_type_from_ext_preserves_html_attachments_for_iframe_publishes() {
        assert_eq!(
            mime_type_from_ext(Path::new("_attachments/audience-filter-demo.html")),
            "text/html"
        );
        assert_eq!(
            mime_type_from_ext(Path::new("_attachments/audience-filter-demo.htm")),
            "text/html"
        );
    }

    // ========================================================================
    // resolve_audiences: dual-read between workspace file and legacy plugin
    // config.
    // ========================================================================

    fn legacy_config_with(states: Vec<(&str, AudienceAccessState)>) -> PublishPluginConfig {
        let mut cfg = PublishPluginConfig::default();
        for (name, state) in states {
            cfg.audience_states.insert(
                name.to_string(),
                AudiencePublishConfig {
                    state,
                    access_method: None,
                },
            );
        }
        cfg
    }

    #[test]
    fn resolve_audiences_uses_workspace_file_when_present() {
        // Even though the legacy HashMap has entries, the file declaration
        // takes priority.
        let file_decls = vec![
            diaryx_core::workspace::AudienceDecl {
                name: "Public".to_string(),
                gates: vec![],
                share_actions: vec![],
            },
            diaryx_core::workspace::AudienceDecl {
                name: "Family".to_string(),
                gates: vec![diaryx_core::workspace::Gate::Link],
                share_actions: vec![],
            },
        ];
        let legacy = legacy_config_with(vec![
            ("LegacyOnly", AudienceAccessState::Public),
            ("Public", AudienceAccessState::AccessControl),
        ]);

        let (resolved, source) = resolve_audiences(Some(&file_decls), &legacy);
        assert_eq!(source, AudienceSource::File);
        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[0].name, "Public");
        assert_eq!(resolved[0].gates, serde_json::json!([]));
        assert!(resolved[0].publish);
        assert_eq!(resolved[1].name, "Family");
        assert_eq!(resolved[1].gates, serde_json::json!([{ "kind": "link" }]));
        // LegacyOnly is intentionally NOT in the resolved set when the file
        // is the source of truth.
        assert!(resolved.iter().all(|a| a.name != "LegacyOnly"));
    }

    #[test]
    fn resolve_audiences_falls_back_to_legacy_when_file_absent() {
        let legacy = legacy_config_with(vec![
            ("Public", AudienceAccessState::Public),
            ("Members", AudienceAccessState::AccessControl),
            ("Hidden", AudienceAccessState::Unpublished),
        ]);

        let (resolved, source) = resolve_audiences(None, &legacy);
        assert_eq!(source, AudienceSource::LegacyPluginConfig);
        assert_eq!(resolved.len(), 3);

        let by_name: std::collections::HashMap<&str, &ResolvedAudience> =
            resolved.iter().map(|a| (a.name.as_str(), a)).collect();

        // Public → empty gates, publishable.
        assert_eq!(by_name["Public"].gates, serde_json::json!([]));
        assert!(by_name["Public"].publish);

        // Members → link gate, publishable.
        assert_eq!(
            by_name["Members"].gates,
            serde_json::json!([{ "kind": "link" }])
        );
        assert!(by_name["Members"].publish);

        // Hidden (Unpublished) → not publishable; the publish flow
        // short-circuits these.
        assert!(!by_name["Hidden"].publish);
    }

    #[test]
    fn resolve_audiences_empty_file_list_yields_empty_resolution() {
        // An explicit empty `audiences: []` block means the writer has
        // declared zero audiences. This is distinct from "no audiences key"
        // (None) — we honor the file source even when empty.
        let legacy = legacy_config_with(vec![("LegacyOnly", AudienceAccessState::Public)]);

        let (resolved, source) = resolve_audiences(Some(&[]), &legacy);
        assert_eq!(source, AudienceSource::File);
        assert!(resolved.is_empty());
    }

    #[test]
    fn resolve_audiences_password_gate_serializes_correctly() {
        let file_decls = vec![diaryx_core::workspace::AudienceDecl {
            name: "Inner".to_string(),
            gates: vec![
                diaryx_core::workspace::Gate::Password,
                diaryx_core::workspace::Gate::Link,
            ],
            share_actions: vec![],
        }];
        let legacy = legacy_config_with(vec![]);

        let (resolved, _) = resolve_audiences(Some(&file_decls), &legacy);
        assert_eq!(
            resolved[0].gates,
            serde_json::json!([
                { "kind": "password" },
                { "kind": "link" }
            ])
        );
    }
}
