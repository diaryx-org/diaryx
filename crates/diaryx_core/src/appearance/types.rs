//! Workspace appearance types.
//!
//! These types represent the resolved appearance settings for a workspace,
//! including theme colors, typography, and favicon. They mirror the frontend
//! `ThemeDefinition` / `TypographySettings` structures and are the canonical
//! Rust representation used by publish, export, and other consumers.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ============================================================================
// Color palette
// ============================================================================

/// Color palette for a single mode (light or dark).
///
/// Maps the app's 26-color OKLch theme palette to the 11 CSS variables used
/// by the publish stylesheet. Values are CSS color strings (OKLch, hex, etc.).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ColorPalette {
    /// Page background (`--bg`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bg: Option<String>,
    /// Primary text color (`--text`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Secondary/muted text (`--text-muted`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_muted: Option<String>,
    /// Accent/link color (`--accent`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent: Option<String>,
    /// Accent hover state (`--accent-hover`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent_hover: Option<String>,
    /// Border color (`--border`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border: Option<String>,
    /// Code/pre background (`--code-bg`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_bg: Option<String>,
    /// Surface background for floating elements (`--surface-bg`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_bg: Option<String>,
    /// Surface border (`--surface-border`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_border: Option<String>,
    /// Surface shadow (`--surface-shadow`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_shadow: Option<String>,
    /// Divider color (`--divider-color`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub divider_color: Option<String>,
}

impl ColorPalette {
    /// Generate CSS variable declarations for all set colors.
    pub fn to_css_vars(&self) -> String {
        let mut vars = String::new();
        let mappings: &[(&Option<String>, &str)] = &[
            (&self.bg, "--bg"),
            (&self.text, "--text"),
            (&self.text_muted, "--text-muted"),
            (&self.accent, "--accent"),
            (&self.accent_hover, "--accent-hover"),
            (&self.border, "--border"),
            (&self.code_bg, "--code-bg"),
            (&self.surface_bg, "--surface-bg"),
            (&self.surface_border, "--surface-border"),
            (&self.surface_shadow, "--surface-shadow"),
            (&self.divider_color, "--divider-color"),
        ];
        for (value, name) in mappings {
            if let Some(v) = value {
                vars.push_str(&format!("    {}: {};\n", name, v));
            }
        }
        vars
    }
}

// ============================================================================
// Typography
// ============================================================================

/// Font family choices matching the frontend `FontFamily` union type.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum FontFamily {
    /// Inter font stack
    Inter,
    /// System font stack (default)
    #[default]
    System,
    /// Serif font stack (Georgia)
    Serif,
    /// Monospace font stack (SF Mono)
    Mono,
}

impl FontFamily {
    /// Map to a CSS `font-family` value, mirroring the frontend `FONT_FAMILY_MAP`.
    pub fn to_css(&self) -> &'static str {
        match self {
            Self::Inter => {
                r#""Inter", -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif"#
            }
            Self::System => {
                r#"-apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif"#
            }
            Self::Serif => r#""Georgia", "Times New Roman", serif"#,
            Self::Mono => r#""SF Mono", Monaco, "Cascadia Code", "Fira Code", monospace"#,
        }
    }

    /// Parse a font family string (as stored in the frontend settings).
    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "inter" => Self::Inter,
            "system" => Self::System,
            "serif" => Self::Serif,
            "mono" => Self::Mono,
            _ => Self::System,
        }
    }
}

/// Content width choices matching the frontend `ContentWidth` union type.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ContentWidth {
    /// Narrow (55ch)
    Narrow,
    /// Medium (65ch, default)
    #[default]
    Medium,
    /// Wide (85ch)
    Wide,
    /// Full width (no max-width)
    Full,
}

impl ContentWidth {
    /// Map to a CSS `max-width` value, mirroring the frontend `CONTENT_WIDTH_MAP`.
    pub fn to_css(&self) -> &'static str {
        match self {
            Self::Narrow => "55ch",
            Self::Medium => "65ch",
            Self::Wide => "85ch",
            Self::Full => "none",
        }
    }

    /// Parse a content width string (as stored in the frontend settings).
    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "narrow" => Self::Narrow,
            "medium" => Self::Medium,
            "wide" => Self::Wide,
            "full" => Self::Full,
            _ => Self::Medium,
        }
    }
}

/// Typography settings for a workspace.
///
/// Mirrors the frontend `TypographySettings` interface.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TypographySettings {
    /// Font family choice.
    #[serde(default)]
    pub font_family: FontFamily,
    /// Base font size in pixels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_font_size: Option<f64>,
    /// Line height multiplier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_height: Option<f64>,
    /// Content max-width choice.
    #[serde(default)]
    pub content_width: ContentWidth,
}

impl TypographySettings {
    /// Generate CSS variable declarations for typography settings.
    pub fn to_css_vars(&self) -> String {
        let mut vars = String::new();

        vars.push_str(&format!(
            "    --font-family: {};\n",
            self.font_family.to_css()
        ));

        if let Some(size) = self.base_font_size {
            vars.push_str(&format!("    --font-size: {}px;\n", size));
        }

        if let Some(lh) = self.line_height {
            vars.push_str(&format!("    --line-height: {};\n", lh));
        }

        vars.push_str(&format!(
            "    --content-max-width: {};\n",
            self.content_width.to_css()
        ));

        vars
    }
}

// ============================================================================
// Favicon
// ============================================================================

/// A favicon asset.
#[derive(Debug, Clone)]
pub struct FaviconAsset {
    /// Filename (e.g. "favicon.svg", "favicon.png", "favicon.ico")
    pub filename: String,
    /// MIME type (e.g. "image/svg+xml", "image/png", "image/x-icon")
    pub mime_type: String,
    /// Raw file bytes
    pub data: Vec<u8>,
}

// ============================================================================
// Theme appearance (combined)
// ============================================================================

/// Resolved workspace appearance: theme colors, typography, and favicon.
///
/// This is the top-level type returned by [`super::resolve_appearance`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ThemeAppearance {
    /// Theme identifier (e.g. "default", "sepia", "nord").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Light mode color palette.
    #[serde(default)]
    pub light: ColorPalette,
    /// Dark mode color palette.
    #[serde(default)]
    pub dark: ColorPalette,
    /// Optional favicon. Not serialized (binary data).
    #[serde(skip)]
    pub favicon: Option<FaviconAsset>,
    /// Typography settings (font, size, line-height, content width).
    #[serde(default)]
    pub typography: Option<TypographySettings>,
}

impl ThemeAppearance {
    /// Generate a CSS block that overrides the default `:root` and dark-mode
    /// variables with theme colors and typography. Returns empty string if
    /// nothing is set.
    pub fn to_css_overrides(&self) -> String {
        let light_vars = self.light.to_css_vars();
        let dark_vars = self.dark.to_css_vars();
        let typo_vars = self
            .typography
            .as_ref()
            .map(|t| t.to_css_vars())
            .unwrap_or_default();

        if light_vars.is_empty() && dark_vars.is_empty() && typo_vars.is_empty() {
            return String::new();
        }

        let mut css = String::new();
        // Typography + light-mode color overrides go in :root
        if !light_vars.is_empty() || !typo_vars.is_empty() {
            css.push_str(&format!(":root {{\n{}{}}}\n", typo_vars, light_vars));
        }
        if !dark_vars.is_empty() {
            css.push_str(&format!(
                "@media (prefers-color-scheme: dark) {{\n  :root {{\n{}\n  }}\n}}\n",
                dark_vars
            ));
        }
        css
    }

    /// Create from an app ThemeDefinition's color palettes.
    ///
    /// Maps the app's semantic color keys to CSS variables:
    /// - background -> bg
    /// - foreground -> text
    /// - muted-foreground -> text-muted
    /// - primary -> accent
    /// - ring -> accent-hover
    /// - border -> border
    /// - secondary -> code-bg
    /// - card -> surface-bg
    /// - sidebar-border -> surface-border
    pub fn from_app_palette(
        light: &HashMap<String, String>,
        dark: &HashMap<String, String>,
    ) -> Self {
        Self {
            id: None,
            light: Self::map_palette(light),
            dark: Self::map_palette(dark),
            favicon: None,
            typography: None,
        }
    }

    /// Generate a fallback SVG favicon from the theme's accent color.
    pub fn generate_favicon_svg(&self) -> FaviconAsset {
        let accent = self.light.accent.as_deref().unwrap_or("#6366f1");
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32"><circle cx="16" cy="16" r="14" fill="{}"/></svg>"#,
            accent
        );
        FaviconAsset {
            filename: "favicon.svg".to_string(),
            mime_type: "image/svg+xml".to_string(),
            data: svg.into_bytes(),
        }
    }

    /// Return the favicon if set, otherwise generate one from the accent color.
    pub fn favicon_or_default(&self) -> FaviconAsset {
        match &self.favicon {
            Some(f) => f.clone(),
            None => self.generate_favicon_svg(),
        }
    }

    fn map_palette(colors: &HashMap<String, String>) -> ColorPalette {
        ColorPalette {
            bg: colors.get("background").cloned(),
            text: colors.get("foreground").cloned(),
            text_muted: colors.get("muted-foreground").cloned(),
            accent: colors.get("primary").cloned(),
            accent_hover: colors.get("ring").cloned(),
            border: colors.get("border").cloned(),
            code_bg: colors.get("secondary").cloned(),
            surface_bg: colors.get("card").cloned(),
            surface_border: colors.get("sidebar-border").cloned(),
            surface_shadow: None,
            divider_color: None,
        }
    }
}
