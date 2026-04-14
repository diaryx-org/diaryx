//! Async resolution of workspace appearance from `.diaryx/` config files.
//!
//! These functions read the workspace's persisted theme and typography settings
//! and return fully-resolved appearance types.

use std::collections::HashMap;
use std::path::Path;

use crate::fs::AsyncFileSystem;

use super::presets::builtin_typography_defaults;
use super::types::*;
use super::{
    FAVICON_CANDIDATES, THEMES_DIR, THEMES_LIBRARY_PATH, THEMES_SETTINGS_PATH,
    TYPOGRAPHIES_LIBRARY_PATH, TYPOGRAPHIES_SETTINGS_PATH,
};

/// Resolve the full workspace appearance: theme colors, typography, and favicon.
///
/// Returns `None` if no theme settings are found (typography and favicon may
/// still be absent on the returned value even when colors are present).
pub async fn resolve_appearance<FS: AsyncFileSystem>(
    fs: &FS,
    workspace_dir: &Path,
) -> Option<ThemeAppearance> {
    let mut theme = resolve_theme_colors(fs, workspace_dir).await?;
    theme.favicon = resolve_favicon(fs, workspace_dir).await;
    theme.typography = resolve_typography(fs, workspace_dir).await;
    Some(theme)
}

/// Resolve theme colors from `.diaryx/themes/{settings,library}.json`.
///
/// Reads the selected preset ID from settings, looks it up in the library,
/// and maps the app's semantic color keys to CSS variable names.
pub async fn resolve_theme_colors<FS: AsyncFileSystem>(
    fs: &FS,
    workspace_dir: &Path,
) -> Option<ThemeAppearance> {
    let settings_path = workspace_dir.join(THEMES_SETTINGS_PATH);
    let settings_str = fs.read_to_string(&settings_path).await.ok()?;
    let settings: serde_json::Value = serde_json::from_str(&settings_str).ok()?;
    let preset_id = settings.get("presetId")?.as_str()?;

    let library_path = workspace_dir.join(THEMES_LIBRARY_PATH);
    let library_str = fs.read_to_string(&library_path).await.ok()?;
    let library: Vec<serde_json::Value> = serde_json::from_str(&library_str).ok()?;

    let theme_def = library.iter().find_map(|entry| {
        let theme = entry.get("theme")?;
        let id = theme.get("id")?.as_str()?;
        if id == preset_id { Some(theme) } else { None }
    })?;

    let colors = theme_def.get("colors")?;
    let light = json_to_color_map(colors.get("light")?);
    let dark = json_to_color_map(colors.get("dark")?);

    Some(ThemeAppearance::from_app_palette(&light, &dark))
}

/// Resolve typography from `.diaryx/typographies/{settings,library}.json`.
///
/// Resolution order: builtin preset defaults -> library preset -> user overrides.
pub async fn resolve_typography<FS: AsyncFileSystem>(
    fs: &FS,
    workspace_dir: &Path,
) -> Option<TypographySettings> {
    let settings_path = workspace_dir.join(TYPOGRAPHIES_SETTINGS_PATH);
    let settings_str = fs.read_to_string(&settings_path).await.ok()?;
    let settings: serde_json::Value = serde_json::from_str(&settings_str).ok()?;
    let preset_id = settings.get("typographyPresetId")?.as_str()?;

    // Start with builtin defaults
    let base = builtin_typography_defaults(preset_id).unwrap_or_default();

    // Try to find the preset in the library (overrides builtins for custom presets)
    let library_path = workspace_dir.join(TYPOGRAPHIES_LIBRARY_PATH);
    let lib_settings = fs
        .read_to_string(&library_path)
        .await
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<serde_json::Value>>(&s).ok())
        .and_then(|library| {
            library.into_iter().find_map(|entry| {
                let typo = entry.get("typography")?;
                let id = typo.get("id")?.as_str()?;
                if id == preset_id {
                    let s = typo.get("settings")?;
                    Some((
                        s.get("fontFamily")
                            .and_then(|v| v.as_str())
                            .map(String::from),
                        s.get("baseFontSize").and_then(|v| v.as_f64()),
                        s.get("lineHeight").and_then(|v| v.as_f64()),
                        s.get("contentWidth")
                            .and_then(|v| v.as_str())
                            .map(String::from),
                    ))
                } else {
                    None
                }
            })
        });

    let (lib_ff, lib_fs, lib_lh, lib_cw) = lib_settings.unwrap_or((None, None, None, None));

    // User overrides trump everything
    let overrides = settings.get("typographyOverrides");

    let font_family_str = overrides
        .and_then(|o| o.get("fontFamily"))
        .and_then(|v| v.as_str())
        .map(String::from)
        .or(lib_ff);

    let font_size = overrides
        .and_then(|o| o.get("baseFontSize"))
        .and_then(|v| v.as_f64())
        .or(lib_fs)
        .or(base.base_font_size);

    let line_height = overrides
        .and_then(|o| o.get("lineHeight"))
        .and_then(|v| v.as_f64())
        .or(lib_lh)
        .or(base.line_height);

    let content_width_str = overrides
        .and_then(|o| o.get("contentWidth"))
        .and_then(|v| v.as_str())
        .map(String::from)
        .or(lib_cw);

    let font_family = font_family_str
        .as_deref()
        .map(FontFamily::from_str_lossy)
        .unwrap_or(base.font_family);

    let content_width = content_width_str
        .as_deref()
        .map(ContentWidth::from_str_lossy)
        .unwrap_or(base.content_width);

    Some(TypographySettings {
        font_family,
        base_font_size: font_size,
        line_height,
        content_width,
    })
}

/// Look for a favicon file in `.diaryx/themes/`.
///
/// Checks for `favicon.svg`, `favicon.png`, and `favicon.ico` in order of
/// preference. Returns `None` if no favicon file is found.
pub async fn resolve_favicon<FS: AsyncFileSystem>(
    fs: &FS,
    workspace_dir: &Path,
) -> Option<FaviconAsset> {
    let themes_dir = workspace_dir.join(THEMES_DIR);
    for (filename, mime_type) in FAVICON_CANDIDATES {
        let path = themes_dir.join(filename);
        if let Ok(data) = fs.read_binary(&path).await {
            return Some(FaviconAsset {
                filename: filename.to_string(),
                mime_type: mime_type.to_string(),
                data,
            });
        }
    }
    None
}

/// Convert a JSON object of color keys to a HashMap.
fn json_to_color_map(palette: &serde_json::Value) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Some(obj) = palette.as_object() {
        for (key, value) in obj {
            if let Some(v) = value.as_str() {
                map.insert(key.clone(), v.to_string());
            }
        }
    }
    map
}
