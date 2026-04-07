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
    /// Whether to send an email digest when publishing to this audience.
    #[serde(default)]
    pub email_on_publish: bool,
    /// Email subject template. Supports `{title}` placeholder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email_subject: Option<String>,
    /// Path to a cover file (markdown) that renders as a personalized intro
    /// above the entry digest in emails.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email_cover: Option<String>,
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
                        // Convert YamlValue to JSON then deserialize
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
        let config_yaml_str = serde_yaml::to_string(&config).map_err(DiaryxError::Yaml)?;
        let config_yaml: diaryx_core::yaml_value::YamlValue =
            serde_yaml::from_str(&config_yaml_str).map_err(DiaryxError::Yaml)?;

        // Store config under `plugins."diaryx.publish"` (dotted key, matching
        // the canonical plugin ID used by the permissions system).
        let plugins_key = "plugins".to_string();
        let plugins_val = fm
            .entry(plugins_key)
            .or_insert_with(|| diaryx_core::yaml_value::YamlValue::Mapping(indexmap::IndexMap::new()));
        if let Some(plugins_map) = plugins_val.as_mapping_mut() {
            // Merge into existing "diaryx.publish" entry (preserves permissions).
            let entry = plugins_map
                .entry("diaryx.publish".into())
                .or_insert_with(|| diaryx_core::yaml_value::YamlValue::Mapping(indexmap::IndexMap::new()));
            if let (Some(existing), Some(config_map)) =
                (entry.as_mapping_mut(), config_yaml.as_mapping())
            {
                for (k, v) in config_map {
                    existing.insert(k.clone(), v.clone());
                }
            }
        }

        let new_content = diaryx_core::frontmatter::serialize(&fm, &parsed.body)?;
        self.fs.write_file(&root, &new_content).await?;
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

    /// Render an email digest for an audience and trigger server-side send.
    ///
    /// 1. Renders cover file (if configured) + entry digest as email HTML
    /// 2. Uploads the draft to `_email_draft/{audience}.html` in the object store
    /// 3. Calls the host's `send_audience_email` to trigger batch sending
    async fn render_and_send_email(
        &self,
        namespace_id: &str,
        audience_name: &str,
        audience_config: &AudiencePublishConfig,
        workspace_root: &Path,
        _rendered_pages: &[crate::publish::RenderedFile],
        format: &dyn crate::publish::PublishFormat,
    ) -> Result<(), String> {
        let default_aud = self.default_audience().await;

        // Collect pages with clean rendered_body (no page wrappers/frontmatter)
        let options = crate::publish::PublishOptions {
            audience: Some(audience_name.to_string()),
            default_audience: default_aud,
            ..Default::default()
        };
        let publisher =
            crate::publish::Publisher::new(self.fs.clone(), &*self.body_renderer, format);
        let pages = publisher
            .collect_pages(workspace_root, &options)
            .await
            .map_err(|e| e.to_string())?;

        // Filter to non-root, non-index pages (actual content entries)
        let content_pages: Vec<&crate::publish::PublishedPage> = pages
            .iter()
            .filter(|p| !p.is_root && !p.contents_links.is_empty() == false)
            .filter(|p| !p.is_root)
            .collect();

        if content_pages.is_empty() && audience_config.email_cover.is_none() {
            return Err("No entries to email and no cover file configured".into());
        }

        // Read and render cover file if configured (strip frontmatter first)
        let cover_html = if let Some(cover_path) = &audience_config.email_cover {
            let cover_full_path = Self::workspace_dir_from_path(workspace_root).join(cover_path);
            match self.fs.read_to_string(&cover_full_path).await {
                Ok(raw) => {
                    // Strip frontmatter before rendering
                    let body = match diaryx_core::frontmatter::parse_or_empty(&raw) {
                        Ok(parsed) => parsed.body,
                        Err(_) => raw,
                    };
                    let preprocessed = format.preprocess_body(&body);
                    Some(format.convert_body(&preprocessed))
                }
                Err(e) => {
                    log::warn!("Failed to read email cover file '{}': {}", cover_path, e);
                    None
                }
            }
        } else {
            None
        };

        // Load theme for email rendering
        let workspace_dir = Self::workspace_dir_from_path(workspace_root);
        let theme = self.load_workspace_theme(&workspace_dir).await;

        // Resolve site title from workspace root entry (same as publisher)
        let site_title = pages
            .first()
            .map(|p| p.title.clone())
            .unwrap_or_else(|| "Newsletter".into());

        // Base URL: prefer subdomain+domain, fall back to site_base_url direct serving
        let base_url = {
            let config = self.config.read().unwrap();
            if let (Some(sub), Some(domain)) = (&config.subdomain, &config.site_domain) {
                Some(format!(
                    "https://{}.{}/{}/index.html",
                    sub, domain, audience_name
                ))
            } else if let Some(site_base) = &config.site_base_url {
                config.namespace_id.as_ref().map(|ns_id| {
                    format!(
                        "{}/sites/{}/{}/index.html",
                        site_base.trim_end_matches('/'),
                        ns_id,
                        audience_name
                    )
                })
            } else {
                None
            }
        };

        // Resolve email subject
        let subject = audience_config
            .email_subject
            .as_deref()
            .unwrap_or("{title} — New posts")
            .replace("{title}", &site_title);

        // Only include entry digest when a site is published (has a URL to link to).
        // Otherwise, send only the cover file content.
        let pages_for_email: Vec<crate::publish::PublishedPage> = if base_url.is_some() {
            content_pages.into_iter().cloned().collect()
        } else {
            vec![]
        };

        // Render the email
        let email_options = crate::publish::email_format::EmailDigestOptions {
            cover_html: cover_html.as_deref(),
            site_title: &site_title,
            base_url: base_url.as_deref(),
            unsubscribe_url: "{unsubscribe_url}",
            theme: theme.as_ref(),
        };
        let email_html =
            crate::publish::email_format::render_email_digest(&pages_for_email, &email_options);

        // Upload draft to object store
        let draft_key = format!("_email_draft/{}.html", audience_name);
        diaryx_plugin_sdk::host::namespace::put_object(
            namespace_id,
            &draft_key,
            email_html.as_bytes(),
            "text/html",
            audience_name,
        )
        .map_err(|e| format!("Failed to upload email draft: {}", e))?;

        // Trigger server-side send
        diaryx_plugin_sdk::host::namespace::send_audience_email(
            namespace_id,
            audience_name,
            &subject,
            None,
        )
        .map_err(|e| format!("Failed to send email: {}", e))?;

        log::info!(
            "Email sent to audience '{}' ({} entries)",
            audience_name,
            pages.len()
        );
        Ok(())
    }

    /// Load the workspace's theme and return an `HtmlFormat` configured with it.
    ///
    /// Reads `.diaryx/themes/settings.json` (for `presetId`) and
    /// `.diaryx/themes/library.json` (for theme definitions). If the theme
    /// files don't exist or the preset isn't found, returns the default format.
    async fn format_with_workspace_theme(&self) -> Arc<dyn PublishFormat> {
        let workspace_dir = match self.current_workspace_dir() {
            Some(root) => root,
            None => return self.format.clone(),
        };

        let theme = match self.load_workspace_theme(&workspace_dir).await {
            Some(t) => t,
            None => return self.format.clone(),
        };

        Arc::new(crate::publish::HtmlFormat::with_theme(theme))
    }

    /// Try to load a `PublishTheme` from workspace appearance files.
    async fn load_workspace_theme(
        &self,
        workspace_dir: &Path,
    ) -> Option<crate::publish::PublishTheme> {
        // Read theme settings (which preset is selected)
        let settings_path = workspace_dir.join(".diaryx/themes/settings.json");
        let settings_str = self.fs.read_to_string(&settings_path).await.ok()?;
        let settings: serde_json::Value = serde_json::from_str(&settings_str).ok()?;
        let preset_id = settings.get("presetId")?.as_str()?;

        // Read theme library (full theme definitions)
        let library_path = workspace_dir.join(".diaryx/themes/library.json");
        let library_str = self.fs.read_to_string(&library_path).await.ok()?;
        let library: Vec<serde_json::Value> = serde_json::from_str(&library_str).ok()?;

        // Find the theme definition matching the preset ID
        let theme_def = library.iter().find_map(|entry| {
            let theme = entry.get("theme")?;
            let id = theme.get("id")?.as_str()?;
            if id == preset_id { Some(theme) } else { None }
        })?;

        // Extract light and dark palettes as HashMaps
        let colors = theme_def.get("colors")?;
        let light = Self::json_to_color_map(colors.get("light")?);
        let dark = Self::json_to_color_map(colors.get("dark")?);

        let mut theme = crate::publish::PublishTheme::from_app_palette(&light, &dark);

        // Look for a user-provided favicon in .diaryx/themes/
        theme.favicon = self.load_workspace_favicon(workspace_dir).await;

        Some(theme)
    }

    /// Look for a favicon file in `.diaryx/themes/`.
    ///
    /// Checks for `favicon.svg`, `favicon.png`, and `favicon.ico` in order of
    /// preference. Returns `None` if no favicon file is found — the theme will
    /// then auto-generate an SVG from its accent color.
    async fn load_workspace_favicon(
        &self,
        workspace_dir: &Path,
    ) -> Option<crate::publish::types::FaviconAsset> {
        let candidates = [
            ("favicon.svg", "image/svg+xml"),
            ("favicon.png", "image/png"),
            ("favicon.ico", "image/x-icon"),
        ];

        let themes_dir = workspace_dir.join(".diaryx/themes");
        for (filename, mime_type) in &candidates {
            let path = themes_dir.join(filename);
            if let Ok(data) = self.fs.read_binary(&path).await {
                return Some(crate::publish::types::FaviconAsset {
                    filename: filename.to_string(),
                    mime_type: mime_type.to_string(),
                    data,
                });
            }
        }

        None
    }

    /// Convert a JSON object of color keys to a HashMap.
    fn json_to_color_map(palette: &serde_json::Value) -> std::collections::HashMap<String, String> {
        let mut map = std::collections::HashMap::new();
        if let Some(obj) = palette.as_object() {
            for (key, value) in obj {
                if let Some(v) = value.as_str() {
                    map.insert(key.clone(), v.to_string());
                }
            }
        }
        map
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
                    "GetPublishConfig".into(),
                    "SetPublishConfig".into(),
                    "GetAudiencePublishStates".into(),
                    "SetAudiencePublishState".into(),
                    "SendEmailToAudience".into(),
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

                // Sync audience access level to the server if namespace is configured.
                let namespace_id = {
                    let config = self.config.read().unwrap();
                    config.namespace_id.clone()
                };
                if let Some(ns_id) = &namespace_id {
                    let access = match state_config.state {
                        AudienceAccessState::Public => "public",
                        AudienceAccessState::AccessControl => "token",
                        AudienceAccessState::Unpublished => "private",
                    };
                    // Best-effort: don't fail the whole command if server sync fails.
                    if let Err(e) =
                        diaryx_plugin_sdk::host::namespace::sync_audience(ns_id, &audience, access)
                    {
                        log::warn!("Failed to sync audience '{}' to server: {}", audience, e);
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

            "PublishToNamespace" => {
                let namespace_id = params["namespace_id"]
                    .as_str()
                    .ok_or_else(|| PluginError::CommandError("missing namespace_id".into()))?
                    .to_string();

                let workspace_root = self
                    .current_root_index_path()
                    .await
                    .map_err(|e| PluginError::CommandError(e.to_string()))?;

                let config = self.config.read().unwrap().clone();
                let default_aud = self.default_audience().await;
                let format = self.format_with_workspace_theme().await;

                let mut audiences_published: Vec<String> = Vec::new();
                let mut files_uploaded: usize = 0;
                let mut files_deleted: usize = 0;
                let mut stale_audiences: Vec<String> = Vec::new();

                // Collect all audience names from config
                let all_audiences: Vec<String> = config.audience_states.keys().cloned().collect();

                for audience_name in &all_audiences {
                    let audience_config = match config.audience_states.get(audience_name) {
                        Some(c) => c,
                        None => continue,
                    };

                    if audience_config.state == AudienceAccessState::Unpublished {
                        // Delete objects for this audience
                        match diaryx_plugin_sdk::host::namespace::list_objects(&namespace_id) {
                            Ok(objects) => {
                                for obj in objects {
                                    if obj.audience.as_deref() == Some(audience_name) {
                                        let _ = diaryx_plugin_sdk::host::namespace::delete_object(
                                            &namespace_id,
                                            &obj.key,
                                        );
                                        files_deleted += 1;
                                    }
                                }
                            }
                            Err(e) => {
                                log::warn!("Failed to list objects for cleanup: {}", e);
                            }
                        }
                        continue;
                    }

                    // Determine access level
                    let access = match audience_config.state {
                        AudienceAccessState::Public => "public",
                        AudienceAccessState::AccessControl => "token",
                        AudienceAccessState::Unpublished => continue,
                    };

                    // Sync audience access level
                    if let Err(e) = diaryx_plugin_sdk::host::namespace::sync_audience(
                        &namespace_id,
                        audience_name,
                        access,
                    ) {
                        return Err(PluginError::CommandError(format!(
                            "failed to sync audience {}: {}",
                            audience_name, e
                        )));
                    }

                    // Render files for this audience
                    let options = crate::publish::PublishOptions {
                        audience: Some(audience_name.clone()),
                        default_audience: default_aud.clone(),
                        ..Default::default()
                    };
                    let publisher = crate::publish::Publisher::new(
                        self.fs.clone(),
                        &*self.body_renderer,
                        &*format,
                    );
                    let (rendered, attachment_paths) = publisher
                        .render_with_attachments(&workspace_root, &options)
                        .await
                        .map_err(|e| PluginError::CommandError(e.to_string()))?;

                    // No entries have this audience tag — remove from config
                    if rendered.is_empty() {
                        log::info!(
                            "Removing stale audience '{}' from publish config: no entries have this tag",
                            audience_name,
                        );
                        stale_audiences.push(audience_name.clone());
                        continue;
                    }

                    // Upload each rendered file
                    let mut uploaded_keys: Vec<String> = Vec::new();
                    for file in &rendered {
                        let key = format!("{}/{}", audience_name, file.path);
                        diaryx_plugin_sdk::host::namespace::put_object(
                            &namespace_id,
                            &key,
                            &file.content,
                            &file.mime_type,
                            audience_name,
                        )
                        .map_err(|e| {
                            PluginError::CommandError(format!(
                                "failed to upload {}: {}",
                                file.path, e
                            ))
                        })?;
                        uploaded_keys.push(key);
                        files_uploaded += 1;
                    }

                    // Upload attachments (images, PDFs, etc.)
                    for (src_path, dest_rel) in &attachment_paths {
                        let key = format!("{}/{}", audience_name, dest_rel.display());
                        if !self.fs.exists(src_path).await {
                            log::warn!(
                                "Skipping attachment {}: source file does not exist",
                                src_path.display()
                            );
                            continue;
                        }
                        match self.fs.read_binary(src_path).await {
                            Ok(bytes) => {
                                let mime = mime_type_from_ext(dest_rel);
                                diaryx_plugin_sdk::host::namespace::put_object(
                                    &namespace_id,
                                    &key,
                                    &bytes,
                                    &mime,
                                    audience_name,
                                )
                                .map_err(|e| {
                                    PluginError::CommandError(format!(
                                        "failed to upload attachment {}: {}",
                                        dest_rel.display(),
                                        e
                                    ))
                                })?;
                                uploaded_keys.push(key);
                                files_uploaded += 1;
                            }
                            Err(e) => {
                                log::warn!("Skipping attachment {}: {}", src_path.display(), e);
                            }
                        }
                    }

                    // Delete stale objects for this audience
                    if let Ok(existing) =
                        diaryx_plugin_sdk::host::namespace::list_objects(&namespace_id)
                    {
                        for obj in existing {
                            if obj.audience.as_deref() == Some(audience_name)
                                && !uploaded_keys.contains(&obj.key)
                            {
                                let _ = diaryx_plugin_sdk::host::namespace::delete_object(
                                    &namespace_id,
                                    &obj.key,
                                );
                                files_deleted += 1;
                            }
                        }
                    }

                    audiences_published.push(audience_name.clone());

                    // Email on publish: render and send email digest
                    if audience_config.email_on_publish {
                        if let Err(e) = self
                            .render_and_send_email(
                                &namespace_id,
                                audience_name,
                                audience_config,
                                &workspace_root,
                                &rendered,
                                &*format,
                            )
                            .await
                        {
                            log::warn!("Email send for audience '{}' failed: {}", audience_name, e);
                        }
                    }
                }

                // Remove stale audiences from config and persist
                if !stale_audiences.is_empty() {
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

                Ok(serde_json::json!({
                    "audiences_published": audiences_published,
                    "files_uploaded": files_uploaded,
                    "files_deleted": files_deleted,
                }))
            }

            "SendEmailToAudience" => {
                let namespace_id = params["namespace_id"]
                    .as_str()
                    .ok_or_else(|| PluginError::CommandError("missing namespace_id".into()))?
                    .to_string();
                let audience_name = params["audience"]
                    .as_str()
                    .ok_or_else(|| PluginError::CommandError("missing audience".into()))?
                    .to_string();

                let workspace_root = self
                    .current_root_index_path()
                    .await
                    .map_err(|e| PluginError::CommandError(e.to_string()))?;

                let config = self.config.read().unwrap().clone();
                let audience_config = config
                    .audience_states
                    .get(&audience_name)
                    .cloned()
                    .unwrap_or_default();

                let format = self.format_with_workspace_theme().await;

                self.render_and_send_email(
                    &namespace_id,
                    &audience_name,
                    &audience_config,
                    &workspace_root,
                    &[],
                    &*format,
                )
                .await
                .map_err(|e| PluginError::CommandError(e))?;

                Ok(serde_json::json!({ "ok": true, "audience": audience_name }))
            }

            _ => Err(PluginError::CommandError(format!(
                "Unknown publish command: {}",
                cmd
            ))),
        }
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

        block_on(
            plugin
                .fs
                .write_file(&root_index, "---\ncontents: []\n---\n"),
        )
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

        block_on(
            plugin
                .fs
                .write_file(&root_index, "---\ncontents: []\n---\n"),
        )
        .expect("root index write should succeed");
        *plugin.workspace_root.write().unwrap() = Some(PathBuf::from("workspace"));

        let resolved =
            block_on(plugin.current_root_index_path()).expect("root index should resolve");
        assert_eq!(resolved, root_index);
    }
}
