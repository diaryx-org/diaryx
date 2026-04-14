//! Builtin typography presets.
//!
//! These mirror the frontend `BUILTIN_TYPOGRAPHY_PRESETS` so that Rust consumers
//! can resolve builtin preset IDs without needing the library file.

use super::types::{ContentWidth, FontFamily, TypographySettings};

/// Return the builtin typography settings for a known preset ID.
///
/// Returns `None` for unknown/custom preset IDs (those must be resolved
/// from the typography library file).
pub fn builtin_typography_defaults(preset_id: &str) -> Option<TypographySettings> {
    match preset_id {
        "default" => Some(TypographySettings {
            font_family: FontFamily::Inter,
            base_font_size: Some(16.0),
            line_height: Some(1.6),
            content_width: ContentWidth::Medium,
        }),
        "editorial-serif" => Some(TypographySettings {
            font_family: FontFamily::Serif,
            base_font_size: Some(18.0),
            line_height: Some(1.8),
            content_width: ContentWidth::Narrow,
        }),
        "compact-system" => Some(TypographySettings {
            font_family: FontFamily::System,
            base_font_size: Some(15.0),
            line_height: Some(1.5),
            content_width: ContentWidth::Wide,
        }),
        "code-notebook" => Some(TypographySettings {
            font_family: FontFamily::Mono,
            base_font_size: Some(15.0),
            line_height: Some(1.6),
            content_width: ContentWidth::Full,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_presets_resolve() {
        let default = builtin_typography_defaults("default").unwrap();
        assert_eq!(default.font_family, FontFamily::Inter);
        assert_eq!(default.base_font_size, Some(16.0));

        let serif = builtin_typography_defaults("editorial-serif").unwrap();
        assert_eq!(serif.font_family, FontFamily::Serif);
        assert_eq!(serif.content_width, ContentWidth::Narrow);

        let compact = builtin_typography_defaults("compact-system").unwrap();
        assert_eq!(compact.font_family, FontFamily::System);

        let mono = builtin_typography_defaults("code-notebook").unwrap();
        assert_eq!(mono.font_family, FontFamily::Mono);
        assert_eq!(mono.content_width, ContentWidth::Full);
    }

    #[test]
    fn unknown_preset_returns_none() {
        assert!(builtin_typography_defaults("custom-thing").is_none());
    }
}
