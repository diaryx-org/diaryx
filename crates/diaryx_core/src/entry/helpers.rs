//! Helper functions for entry operations.
//!
//! This module contains utility functions for working with filenames and titles.

use crate::workspace::FilenameStyle;

/// Characters that are illegal in filenames on major filesystems (Windows, macOS, Linux).
const FS_ILLEGAL_CHARS: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

/// Convert a filename to a prettier title.
/// e.g., "my-note" -> "My Note", "some_file" -> "Some File"
pub fn prettify_filename(filename: &str) -> String {
    filename
        .replace(['-', '_'], " ")
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars).collect(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Slugify a string for use in URLs and filenames.
/// Converts to lowercase, replaces non-alphanumeric with dashes, removes consecutive dashes.
/// e.g., "My Cool Entry!" -> "my-cool-entry"
pub fn slugify(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Convert a title to a kebab-case filename with .md extension.
/// e.g., "My Cool Entry" -> "my-cool-entry.md"
/// Handles unicode, special characters, and multiple spaces.
pub fn slugify_title(title: &str) -> String {
    let slug = slugify(title);
    if slug.is_empty() {
        "untitled.md".to_string()
    } else {
        format!("{}.md", slug)
    }
}

/// Apply a filename style to a title, returning the filename stem (without extension).
///
/// - `Preserve`: Strip only filesystem-illegal characters, keep spaces/caps/unicode.
/// - `KebabCase`: Lowercase, non-alphanumeric → dashes, collapse consecutive.
/// - `SnakeCase`: Lowercase, non-alphanumeric → underscores, collapse consecutive.
/// - `ScreamingSnakeCase`: Uppercase, non-alphanumeric → underscores, collapse consecutive.
pub fn apply_filename_style(title: &str, style: &FilenameStyle) -> String {
    match style {
        FilenameStyle::Preserve => {
            let cleaned: String = title
                .chars()
                .filter(|c| !FS_ILLEGAL_CHARS.contains(c))
                .collect();
            let trimmed = cleaned.trim();
            if trimmed.is_empty() {
                "Untitled".to_string()
            } else {
                trimmed.to_string()
            }
        }
        FilenameStyle::KebabCase => slugify(title),
        FilenameStyle::SnakeCase => {
            let result: String = title
                .to_lowercase()
                .chars()
                .map(|c| if c.is_alphanumeric() { c } else { '_' })
                .collect();
            let collapsed: String = result
                .split('_')
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("_");
            if collapsed.is_empty() {
                "untitled".to_string()
            } else {
                collapsed
            }
        }
        FilenameStyle::ScreamingSnakeCase => {
            let result: String = title
                .to_uppercase()
                .chars()
                .map(|c| if c.is_alphanumeric() { c } else { '_' })
                .collect();
            let collapsed: String = result
                .split('_')
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("_");
            if collapsed.is_empty() {
                "UNTITLED".to_string()
            } else {
                collapsed
            }
        }
    }
}

/// Convert a title to a filename with .md extension, using the given filename style.
#[allow(dead_code)]
pub fn slugify_title_with_style(title: &str, style: &FilenameStyle) -> String {
    let stem = apply_filename_style(title, style);
    format!("{}.md", stem)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prettify_filename() {
        assert_eq!(prettify_filename("my-note"), "My Note");
        assert_eq!(prettify_filename("some_file"), "Some File");
        assert_eq!(prettify_filename("already-cool"), "Already Cool");
    }

    #[test]
    fn test_slugify_title() {
        assert_eq!(slugify_title("My Cool Entry"), "my-cool-entry.md");
        assert_eq!(slugify_title("Hello World!"), "hello-world.md");
        assert_eq!(slugify_title("  spaces  "), "spaces.md");
        assert_eq!(slugify_title(""), "untitled.md");
    }

    #[test]
    fn test_preserve_style() {
        assert_eq!(
            apply_filename_style("My Entry: A Story", &FilenameStyle::Preserve),
            "My Entry A Story"
        );
        assert_eq!(
            apply_filename_style("Hello World!", &FilenameStyle::Preserve),
            "Hello World!"
        );
        assert_eq!(
            apply_filename_style("café notes", &FilenameStyle::Preserve),
            "café notes"
        );
        assert_eq!(
            apply_filename_style("file/with\\bad:chars", &FilenameStyle::Preserve),
            "filewithbadchars"
        );
        assert_eq!(
            apply_filename_style("", &FilenameStyle::Preserve),
            "Untitled"
        );
        assert_eq!(
            apply_filename_style("***", &FilenameStyle::Preserve),
            "Untitled"
        );
    }

    #[test]
    fn test_kebab_case_style() {
        assert_eq!(
            apply_filename_style("My Cool Entry", &FilenameStyle::KebabCase),
            "my-cool-entry"
        );
        assert_eq!(
            apply_filename_style("Hello World!", &FilenameStyle::KebabCase),
            "hello-world"
        );
    }

    #[test]
    fn test_snake_case_style() {
        assert_eq!(
            apply_filename_style("My Cool Entry", &FilenameStyle::SnakeCase),
            "my_cool_entry"
        );
        assert_eq!(
            apply_filename_style("Hello World!", &FilenameStyle::SnakeCase),
            "hello_world"
        );
        assert_eq!(
            apply_filename_style("", &FilenameStyle::SnakeCase),
            "untitled"
        );
    }

    #[test]
    fn test_screaming_snake_case_style() {
        assert_eq!(
            apply_filename_style("My Cool Entry", &FilenameStyle::ScreamingSnakeCase),
            "MY_COOL_ENTRY"
        );
        assert_eq!(
            apply_filename_style("Hello World!", &FilenameStyle::ScreamingSnakeCase),
            "HELLO_WORLD"
        );
        assert_eq!(
            apply_filename_style("", &FilenameStyle::ScreamingSnakeCase),
            "UNTITLED"
        );
    }

    #[test]
    fn test_slugify_title_with_style() {
        assert_eq!(
            slugify_title_with_style("My Entry", &FilenameStyle::Preserve),
            "My Entry.md"
        );
        assert_eq!(
            slugify_title_with_style("My Entry", &FilenameStyle::KebabCase),
            "my-entry.md"
        );
        assert_eq!(
            slugify_title_with_style("My Entry", &FilenameStyle::SnakeCase),
            "my_entry.md"
        );
        assert_eq!(
            slugify_title_with_style("My Entry", &FilenameStyle::ScreamingSnakeCase),
            "MY_ENTRY.md"
        );
    }
}
