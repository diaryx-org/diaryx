//! Workspace configuration: reading, migrating, and writing the
//! `workspace_config` settings (root-index frontmatter or linked `Config.md`).

use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::error::{DiaryxError, Result};
use crate::fs::AsyncFileSystem;
use crate::link_parser::{self, LinkFormat};
use crate::yaml;

use super::*;

impl<FS: AsyncFileSystem> Workspace<FS> {
    /// Read workspace configuration from the root index file.
    ///
    /// Supports three storage modes (checked in order):
    /// 1. **File link** — `workspace_config: "[Config](/Meta/Config.md)"`
    /// 2. **Nested section** — `workspace_config:` mapping in frontmatter
    /// 3. **Legacy flat** — config fields at the top level of frontmatter
    pub async fn get_workspace_config(&self, root_index_path: &Path) -> Result<WorkspaceConfig> {
        let (config_extra, _config_path) = self.resolve_config_source(root_index_path).await?;

        // Also load the root index extra for backward-compat fallback
        // (fields that haven't been migrated yet may still be at the top level).
        let index = self.parse_index(root_index_path).await?;
        let root_extra = &index.frontmatter.extra;

        // config_get with nested=None since config_extra IS the resolved source.
        // We look in config_extra first, then fall back to root_extra.
        let get = |key: &str| -> Option<&yaml::Value> {
            config_extra.get(key).or_else(|| root_extra.get(key))
        };

        let link_format = get("link_format")
            .and_then(|v| v.as_str())
            .and_then(|s| match s {
                "markdown_root" => Some(LinkFormat::MarkdownRoot),
                "markdown_relative" => Some(LinkFormat::MarkdownRelative),
                "plain_relative" => Some(LinkFormat::PlainRelative),
                "plain_canonical" => Some(LinkFormat::PlainCanonical),
                _ => None,
            })
            .unwrap_or_default();

        let default_template = get("default_template")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let sync_title_to_heading = get("sync_title_to_heading")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let auto_update_timestamp = get("auto_update_timestamp")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let auto_rename_to_title = get("auto_rename_to_title")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let filename_style = get("filename_style")
            .and_then(|v| v.as_str())
            .and_then(|s| match s {
                "preserve" => Some(FilenameStyle::Preserve),
                "kebab_case" => Some(FilenameStyle::KebabCase),
                "snake_case" => Some(FilenameStyle::SnakeCase),
                "screaming_snake_case" => Some(FilenameStyle::ScreamingSnakeCase),
                _ => None,
            })
            .unwrap_or_default();

        let default_audience = get("default_audience")
            .or_else(|| get("public_audience"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let daily_entry_folder = get("daily_entry_folder")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let show_unlinked_files = get("show_unlinked_files").and_then(|v| v.as_bool());

        let show_hidden_files = get("show_hidden_files").and_then(|v| v.as_bool());

        let theme_mode = get("theme_mode")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let theme_preset = get("theme_preset")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let theme_accent_hue = get("theme_accent_hue").and_then(|v| v.as_f64());

        let audience_colors = get("audience_colors")
            .and_then(|v| v.as_mapping())
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                    .collect()
            });

        let disabled_plugins = get("disabled_plugins")
            .and_then(|v| v.as_sequence())
            .map(|seq| {
                seq.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            });

        // Per-plugin config (install records + granted permissions). Kept as a
        // raw mapping; only present when at least one plugin has an entry. The
        // `get` helper already falls back from the linked config file to the
        // root index, so this reads correctly across migration states.
        let plugins = get("plugins")
            .filter(|v| matches!(v, yaml::Value::Mapping(_)))
            .cloned();

        // Publishing config lives under the top-level `publish:` key. Parse it
        // via fig's `FromValue`; a malformed shape drops to `None` rather than
        // accepting partial garbage.
        let mut publish = get("publish").and_then(|v| {
            <PublishSettings as fig::FromValue>::from_value(&fig::ToValue::to_value(v)).ok()
        });

        // Backward compatibility: fold in the pre-migration locations — the
        // top-level `audiences`/`audiences_migrated` keys and the former
        // `plugins."diaryx.publish".config` blob. The eager migration on open
        // relocates these permanently; this keeps reads correct before it runs
        // (and on the server, which reads config without migrating).
        let legacy_audiences = get("audiences").and_then(|v| {
            <Vec<AudienceDecl> as fig::FromValue>::from_value(&fig::ToValue::to_value(v)).ok()
        });
        let legacy_audiences_migrated = get("audiences_migrated").and_then(|v| v.as_bool());
        let legacy_plugin_cfg = get("plugins")
            .and_then(|p| p.get("diaryx.publish"))
            .and_then(|e| e.get("config"))
            .cloned();

        if publish.is_none()
            && (legacy_audiences.is_some()
                || legacy_audiences_migrated.is_some()
                || legacy_plugin_cfg.is_some())
        {
            publish = Some(PublishSettings::default());
        }
        if let Some(p) = publish.as_mut() {
            if p.audiences.is_none() {
                p.audiences = legacy_audiences;
            }
            if p.audiences_migrated.is_none() {
                p.audiences_migrated = legacy_audiences_migrated;
            }
            if let Some(cfg) = legacy_plugin_cfg {
                if p.namespace_id.is_none() {
                    p.namespace_id = cfg
                        .get("namespace_id")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                }
                if p.subdomain.is_none() {
                    p.subdomain = cfg
                        .get("subdomain")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                }
                if p.audience_states.is_none() {
                    p.audience_states = cfg.get("audience_states").cloned();
                }
                if p.public_audiences.is_none() {
                    p.public_audiences = cfg.get("public_audiences").and_then(|v| {
                        <Vec<String> as fig::FromValue>::from_value(&fig::ToValue::to_value(v)).ok()
                    });
                }
            }
        }

        Ok(WorkspaceConfig {
            link_format,
            default_template,
            sync_title_to_heading,
            auto_update_timestamp,
            auto_rename_to_title,
            filename_style,
            default_audience,
            daily_entry_folder,
            show_unlinked_files,
            show_hidden_files,
            theme_mode,
            theme_preset,
            theme_accent_hue,
            audience_colors,
            disabled_plugins,
            plugins,
            publish,
        })
    }

    /// Convert a string value to a YAML Value, handling booleans and JSON.
    pub(crate) fn parse_config_value(value: &str) -> yaml::Value {
        match value {
            "true" => yaml::Value::Bool(true),
            "false" => yaml::Value::Bool(false),
            _ => {
                // Try parsing as JSON for scalars and complex types.
                if let Ok(json) = crate::yaml::parse_json(value) {
                    match &json {
                        yaml::Value::String(_) => yaml::Value::String(value.to_string()),
                        _ => json,
                    }
                } else {
                    yaml::Value::String(value.to_string())
                }
            }
        }
    }

    /// Default workspace-relative location for the settings file. Used only
    /// when establishing a new settings file; an existing `workspace_config`
    /// link is always followed instead, so the file is not required to live
    /// here.
    const DEFAULT_CONFIG_LINK_REL: &'static str = "Meta/Config.md";

    /// Default scaffold written when the settings file is first created.
    const DEFAULT_CONFIG_FILE: &'static str = "---\n\
title: Workspace Settings\n\
description: Diaryx settings for this workspace. Linked from the root index; not part of your content and not published.\n\
---\n\
\n\
# Workspace Settings\n\
\n\
Diaryx stores this workspace's settings here. Edit them from the Diaryx settings UI \
rather than by hand. This file is referenced by the root index via `workspace_config` \
and is intentionally kept out of the content hierarchy.\n";

    /// Resolve a link stored in the root index to an absolute filesystem path,
    /// honoring every link format. The root index lives at the workspace root,
    /// so its directory *is* the workspace root — which means root-relative
    /// (`/Meta/...`), explicit-relative, and plain links all resolve correctly
    /// by joining the parsed path onto the index's directory. (This is what
    /// [`crate::workspace::types::IndexFile::resolve_path`] gets wrong for
    /// workspace-root links, which it leaves un-joined.)
    pub(crate) fn resolve_root_index_link(&self, index: &IndexFile, link: &str) -> PathBuf {
        let parsed = link_parser::parse_link(link);
        let dir = index.directory().unwrap_or_else(|| Path::new(""));
        crate::path_utils::normalize_path(&dir.join(&parsed.path))
    }

    /// Default absolute path of the settings file for a given root index, used
    /// when no `workspace_config` link exists yet.
    pub(crate) fn default_config_file_path(&self, root_index_path: &Path) -> PathBuf {
        root_index_path
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .join(Self::DEFAULT_CONFIG_LINK_REL)
    }

    /// The `workspace_config` link string the root index uses to point at the
    /// settings file, formatted with the workspace's `link_format`.
    pub(crate) fn config_link_string(
        &self,
        root_index_path: &Path,
        link_format: LinkFormat,
    ) -> String {
        let from = root_index_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "README.md".to_string());
        link_parser::format_link_with_format(
            Self::DEFAULT_CONFIG_LINK_REL,
            "Config",
            link_format,
            &from,
        )
    }

    /// Gather config values stored inline in the root index — from the flat
    /// top-level fields and/or a nested `workspace_config` mapping. The nested
    /// section wins over flat (matching read precedence) and contributes any
    /// keys not in the canonical field list.
    pub(crate) fn collect_inline_config(
        extra: &std::collections::HashMap<String, yaml::Value>,
    ) -> indexmap::IndexMap<String, yaml::Value> {
        let mut collected = indexmap::IndexMap::new();
        // Flat top-level config fields, in canonical order for determinism.
        for field in Self::WORKSPACE_CONFIG_FIELDS {
            if let Some(v) = extra.get(*field) {
                collected.insert((*field).to_string(), v.clone());
            }
        }
        // A nested mapping overrides flat values and contributes extra keys.
        if let Some(yaml::Value::Mapping(map)) = extra.get("workspace_config") {
            for (k, v) in map {
                collected.insert(k.clone(), v.clone());
            }
        }
        collected
    }

    /// Write `collected` into the settings file, creating it from the default
    /// scaffold when missing and merging without clobbering existing keys
    /// (values already in the file win, so re-running is safe).
    ///
    /// The frontmatter is parsed, merged, and re-serialized as a whole rather
    /// than spliced key-by-key: the config values include nested block
    /// sequences (`audiences`), and incremental text splicing can misplace a
    /// scalar written after a block. The settings file is machine-managed, so
    /// a structural rewrite (which doesn't preserve hand-written comments) is
    /// an acceptable trade for correctness here — unlike the root index, which
    /// stays comment-preserving.
    pub(crate) async fn write_config_file(
        &self,
        config_path: &Path,
        collected: &indexmap::IndexMap<String, yaml::Value>,
    ) -> Result<()> {
        if let Some(parent) = config_path.parent() {
            self.fs.create_dir_all(parent).await?;
        }

        let content = match self.fs.read_to_string(config_path).await {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Self::DEFAULT_CONFIG_FILE.to_string()
            }
            Err(e) => {
                return Err(DiaryxError::FileRead {
                    path: config_path.to_path_buf(),
                    source: e,
                });
            }
        };

        let parsed = crate::frontmatter::parse_or_empty(&content)?;
        let mut frontmatter = parsed.frontmatter;

        // Emit nested block sequences (e.g. `audiences`) LAST. The `fig` YAML
        // backend serializes block-sequence items at parent indentation, and
        // its parser then silently drops a top-level key that *follows* such a
        // block. Keeping nested sequences at the end means nothing follows them,
        // so the round-trip is lossless. Existing keys win (merge-not-clobber).
        let is_nested_seq = |v: &yaml::Value| {
            matches!(v, yaml::Value::Sequence(items)
                if items.iter().any(|e| matches!(e, yaml::Value::Mapping(_) | yaml::Value::Sequence(_))))
        };
        for (key, val) in collected.iter().filter(|(_, v)| !is_nested_seq(v)) {
            frontmatter
                .entry(key.clone())
                .or_insert_with(|| val.clone());
        }
        for (key, val) in collected.iter().filter(|(_, v)| is_nested_seq(v)) {
            frontmatter
                .entry(key.clone())
                .or_insert_with(|| val.clone());
        }

        let new_content = crate::frontmatter::serialize(&frontmatter, &parsed.body)?;
        self.fs
            .write(config_path, new_content.as_bytes())
            .await
            .map_err(|e| DiaryxError::FileWrite {
                path: config_path.to_path_buf(),
                source: e,
            })
    }

    /// Remove any flat top-level config fields from the root index, leaving the
    /// `workspace_config` link (and everything else) intact. Comment-preserving.
    ///
    /// Used to sweep fields that linger inline in an already-linked workspace —
    /// e.g. a field newly added to [`Self::WORKSPACE_CONFIG_FIELDS`] after the
    /// workspace was first migrated.
    pub(crate) async fn strip_inline_config_from_root(&self, root_index_path: &Path) -> Result<()> {
        let mut content =
            self.fs
                .read_to_string(root_index_path)
                .await
                .map_err(|e| DiaryxError::FileRead {
                    path: root_index_path.to_path_buf(),
                    source: e,
                })?;

        for field in Self::WORKSPACE_CONFIG_FIELDS {
            content = crate::frontmatter::remove_property_in_text(&content, field)?;
        }

        self.fs
            .write(root_index_path, content.as_bytes())
            .await
            .map_err(|e| DiaryxError::FileWrite {
                path: root_index_path.to_path_buf(),
                source: e,
            })
    }

    /// Strip inline config from the root index and replace it with a single
    /// `workspace_config` link to the settings file, formatted with the
    /// workspace's `link_format`. Comment-preserving.
    pub(crate) async fn rewrite_root_config_link(
        &self,
        root_index_path: &Path,
        link_format: LinkFormat,
    ) -> Result<()> {
        let mut content =
            self.fs
                .read_to_string(root_index_path)
                .await
                .map_err(|e| DiaryxError::FileRead {
                    path: root_index_path.to_path_buf(),
                    source: e,
                })?;

        // Remove any flat config fields and the nested mapping (no-op if absent).
        for field in Self::WORKSPACE_CONFIG_FIELDS {
            content = crate::frontmatter::remove_property_in_text(&content, field)?;
        }
        content = crate::frontmatter::remove_property_in_text(&content, "workspace_config")?;

        // Insert the link to the settings file.
        let link = yaml::Value::String(self.config_link_string(root_index_path, link_format));
        content = crate::frontmatter::set_property_in_text(&content, "workspace_config", &link)?;

        self.fs
            .write(root_index_path, content.as_bytes())
            .await
            .map_err(|e| DiaryxError::FileWrite {
                path: root_index_path.to_path_buf(),
                source: e,
            })
    }

    /// Migrate a workspace's settings out of the root index and into a linked
    /// `Meta/Config.md` settings file.
    ///
    /// Handles both legacy storage modes — flat top-level fields and the nested
    /// `workspace_config` mapping — moving them into the settings file and
    /// replacing them with a single link. Idempotent: returns `Ok(false)`
    /// (without touching any files) when the root index already links to a
    /// settings file, or when there is no inline config to move. Returns
    /// `Ok(true)` when a migration was performed.
    pub async fn migrate_workspace_config_to_file(&self, root_index_path: &Path) -> Result<bool> {
        let index = self.parse_index(root_index_path).await?;
        let wc = index.frontmatter.extra.get("workspace_config");

        // Already linked: the bulk of config already lives in the settings file,
        // but a field added to WORKSPACE_CONFIG_FIELDS *after* this workspace was
        // first migrated (e.g. `plugins`) can still linger inline in the root
        // index. Sweep any such lingering fields into the linked file and strip
        // them from the root, leaving the existing link untouched. No lingering
        // fields → genuinely nothing to do.
        if let Some(yaml::Value::String(link_str)) = wc {
            let lingering = Self::collect_inline_config(&index.frontmatter.extra);
            if lingering.is_empty() {
                return Ok(false);
            }
            let config_path = self.resolve_root_index_link(&index, link_str);
            self.write_config_file(&config_path, &lingering).await?;
            self.strip_inline_config_from_root(root_index_path).await?;
            return Ok(true);
        }

        let collected = Self::collect_inline_config(&index.frontmatter.extra);
        let has_nested = matches!(wc, Some(yaml::Value::Mapping(_)));

        // A clean workspace with no inline config is left untouched, so we don't
        // litter brand-new workspaces with an empty settings file.
        if collected.is_empty() && !has_nested {
            return Ok(false);
        }

        // Read the workspace's link format from its current (inline) location so
        // the new `workspace_config` link is written in the same style.
        let link_format = self.get_link_format(root_index_path).await?;

        let config_path = self.default_config_file_path(root_index_path);
        self.write_config_file(&config_path, &collected).await?;
        self.rewrite_root_config_link(root_index_path, link_format)
            .await?;
        Ok(true)
    }

    /// Relocate legacy publish config into the top-level `publish:` section of
    /// the settings file: the former `plugins."diaryx.publish".config` blob
    /// (`namespace_id`, `subdomain`, `audience_states`, `public_audiences`) and
    /// the previously top-level `audiences`/`audiences_migrated` keys.
    ///
    /// Idempotent: returns `Ok(false)` when there is nothing to relocate (no
    /// settings file, or the legacy keys are already gone). Run after
    /// [`Self::migrate_workspace_config_to_file`] so all config lives in the
    /// settings file first.
    pub async fn migrate_publish_config(&self, root_index_path: &Path) -> Result<bool> {
        let (_extra, config_path) = self.resolve_config_source(root_index_path).await?;
        // No linked settings file → nothing to relocate (clean/new workspace).
        let Some(config_path) = config_path else {
            return Ok(false);
        };

        let content = match self.fs.read_to_string(&config_path).await {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => {
                return Err(DiaryxError::FileRead {
                    path: config_path,
                    source: e,
                });
            }
        };
        let parsed = crate::frontmatter::parse_or_empty(&content)?;
        let mut fm = parsed.frontmatter;

        let legacy_audiences = fm.shift_remove("audiences");
        let legacy_migrated = fm.shift_remove("audiences_migrated");
        let plugin_cfg = fm
            .get("plugins")
            .and_then(|p| p.get("diaryx.publish"))
            .and_then(|e| e.get("config"))
            .cloned();

        // Nothing to relocate → leave the file untouched (idempotent).
        if legacy_audiences.is_none() && legacy_migrated.is_none() && plugin_cfg.is_none() {
            return Ok(false);
        }

        fn set_if_absent(map: &mut yaml::Mapping, key: &str, val: Option<yaml::Value>) {
            if let Some(v) = val
                && !map.contains_key(key)
            {
                map.insert(key.to_string(), v);
            }
        }

        // Merge onto any existing `publish` mapping (existing values win).
        let mut publish = match fm.shift_remove("publish") {
            Some(yaml::Value::Mapping(m)) => m,
            _ => yaml::Mapping::new(),
        };
        if let Some(cfg) = &plugin_cfg {
            set_if_absent(
                &mut publish,
                "namespace_id",
                cfg.get("namespace_id").cloned(),
            );
            set_if_absent(&mut publish, "subdomain", cfg.get("subdomain").cloned());
            set_if_absent(
                &mut publish,
                "audience_states",
                cfg.get("audience_states").cloned(),
            );
            set_if_absent(
                &mut publish,
                "public_audiences",
                cfg.get("public_audiences").cloned(),
            );
        }
        set_if_absent(&mut publish, "audiences_migrated", legacy_migrated);
        set_if_absent(&mut publish, "audiences", legacy_audiences);

        // Drop the former plugin entry (remove `plugins` entirely if now empty).
        if let Some(yaml::Value::Mapping(mut plugins)) = fm.shift_remove("plugins") {
            plugins.shift_remove("diaryx.publish");
            if !plugins.is_empty() {
                fm.insert("plugins".to_string(), yaml::Value::Mapping(plugins));
            }
        }

        // Keep the nested `audiences` block sequence LAST within `publish`: fig's
        // YAML parser drops a key that follows a block sequence at the same
        // level, so nothing may come after it.
        if let Some(aud) = publish.shift_remove("audiences") {
            publish.insert("audiences".to_string(), aud);
        }

        // Likewise `publish` (which contains that nested sequence) must be the
        // last top-level key in the settings file.
        fm.insert("publish".to_string(), yaml::Value::Mapping(publish));

        let new_content = crate::frontmatter::serialize(&fm, &parsed.body)?;
        self.fs
            .write(&config_path, new_content.as_bytes())
            .await
            .map_err(|e| DiaryxError::FileWrite {
                path: config_path,
                source: e,
            })?;
        Ok(true)
    }

    /// Set a workspace configuration field.
    ///
    /// Settings live in a linked settings file (defaulting to `Meta/Config.md`):
    /// - If the root index already links to one, the field is written to
    ///   whatever file the `workspace_config` link points at.
    /// - Otherwise the settings file is established (migrating any inline
    ///   config first), the root index is repointed to it, and the field is
    ///   written to the settings file.
    pub async fn set_workspace_config_field(
        &self,
        root_index_path: &Path,
        field: &str,
        value: &str,
    ) -> Result<()> {
        let yaml_value = Self::parse_config_value(value);
        self.set_workspace_config_field_value(root_index_path, field, yaml_value)
            .await
    }

    /// Set a workspace configuration field to an already-built YAML value.
    ///
    /// Like [`Self::set_workspace_config_field`] but takes a `yaml::Value`
    /// directly, for callers that build structured (non-scalar) values such
    /// as the nested `plugins` mapping. Settings live in a linked settings
    /// file (defaulting to `Meta/Config.md`), establishing and migrating it on
    /// first write as needed.
    pub async fn set_workspace_config_field_value(
        &self,
        root_index_path: &Path,
        field: &str,
        yaml_value: yaml::Value,
    ) -> Result<()> {
        let index = self.parse_index(root_index_path).await?;
        if let Some(yaml::Value::String(link_str)) = index.frontmatter.extra.get("workspace_config")
        {
            // Already linked: write straight to the settings file the link
            // points at (any link format).
            let config_path = self.resolve_root_index_link(&index, link_str);
            return self
                .set_frontmatter_property(&config_path, field, yaml_value)
                .await;
        }

        // Establish the linked settings file (migrating any inline config), then
        // write the field there. An empty workspace gets the scaffold + link on
        // its first config write. The link follows the workspace's link format.
        let link_format = self.get_link_format(root_index_path).await?;
        let collected = Self::collect_inline_config(&index.frontmatter.extra);
        let config_path = self.default_config_file_path(root_index_path);
        self.write_config_file(&config_path, &collected).await?;
        self.rewrite_root_config_link(root_index_path, link_format)
            .await?;
        self.set_frontmatter_property(&config_path, field, yaml_value)
            .await
    }

    /// Read a single plugin's declarative config (`plugins.<id>.config`) from
    /// the workspace settings file. Returns `None` if the plugin has no
    /// `config` entry.
    pub async fn get_workspace_plugin_config(
        &self,
        root_index_path: &Path,
        plugin_id: &str,
    ) -> Result<Option<yaml::Value>> {
        let (source, _) = self.resolve_config_source(root_index_path).await?;
        let config = source
            .get("plugins")
            .and_then(|plugins| plugins.get(plugin_id))
            .and_then(|entry| entry.get("config"))
            .cloned();
        Ok(config)
    }

    /// Set a single plugin's declarative config under `plugins.<id>.config` in
    /// the workspace settings file.
    ///
    /// Read-modify-writes the whole `plugins` mapping, preserving every other
    /// plugin's entry and this plugin's sibling subkeys (`download`,
    /// `permissions`). Plugin *state/blobs* stay in `host::storage`; only
    /// declarative, user-editable settings belong here.
    pub async fn set_workspace_plugin_config(
        &self,
        root_index_path: &Path,
        plugin_id: &str,
        config: yaml::Value,
    ) -> Result<()> {
        let (source, _) = self.resolve_config_source(root_index_path).await?;
        let mut plugins = match source.get("plugins") {
            Some(yaml::Value::Mapping(m)) => m.clone(),
            _ => yaml::Mapping::new(),
        };
        let mut entry = match plugins.get(plugin_id) {
            Some(yaml::Value::Mapping(m)) => m.clone(),
            _ => yaml::Mapping::new(),
        };
        entry.insert("config".to_string(), config);
        plugins.insert(plugin_id.to_string(), yaml::Value::Mapping(entry));
        self.set_workspace_config_field_value(
            root_index_path,
            "plugins",
            yaml::Value::Mapping(plugins),
        )
        .await
    }

    /// Get the link format configuration from a workspace root index.
    pub async fn get_link_format(&self, root_index_path: &Path) -> Result<LinkFormat> {
        let config = self.get_workspace_config(root_index_path).await?;
        Ok(config.link_format)
    }

    /// Set the link format configuration in a workspace root index.
    pub async fn set_link_format(&self, root_index_path: &Path, format: LinkFormat) -> Result<()> {
        let format_str = match format {
            LinkFormat::MarkdownRoot => "markdown_root",
            LinkFormat::MarkdownRelative => "markdown_relative",
            LinkFormat::PlainRelative => "plain_relative",
            LinkFormat::PlainCanonical => "plain_canonical",
        };
        self.set_workspace_config_field(root_index_path, "link_format", format_str)
            .await
    }

    /// Resolve workspace: check current dir, then fall back to config default
    pub async fn resolve_workspace(&self, current_dir: &Path, config: &Config) -> Result<PathBuf> {
        // First, try to detect workspace in current directory
        if let Some(root) = self.detect_workspace(current_dir).await? {
            return Ok(root);
        }

        // Fall back to config's default_workspace and look for root index there
        if let Some(root) = self
            .find_root_index_in_dir(&config.default_workspace)
            .await?
        {
            return Ok(root);
        }

        // If no root index exists in default_workspace, return the expected README.md path
        // (it may need to be created)
        Ok(config.default_workspace.join("README.md"))
    }

    /// Initialize a new workspace with a root index file
    pub async fn init_workspace(
        &self,
        dir: &Path,
        title: Option<&str>,
        description: Option<&str>,
    ) -> Result<PathBuf> {
        // Check if ANY root index already exists in this directory
        // (not just README.md - could be index.md or any other .md file)
        if let Ok(Some(existing_root)) = self.find_root_index_in_dir(dir).await {
            return Err(DiaryxError::WorkspaceAlreadyExists(existing_root));
        }

        let readme_path = dir.join("README.md");

        // Create directory if needed
        self.fs.create_dir_all(dir).await?;

        let display_title = title.unwrap_or_else(|| {
            dir.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Workspace")
        });

        let desc = description.unwrap_or("A diaryx workspace");

        let content = format!(
            "---\ntitle: {}\ndescription: {}\ncontents: []\n---\n\n# {}\n\n{}\n",
            display_title, desc, display_title, desc
        );

        self.fs
            .create_new(&readme_path, content.as_bytes())
            .await
            .map_err(|e| DiaryxError::FileWrite {
                path: readme_path.clone(),
                source: e,
            })?;

        Ok(readme_path)
    }
}
