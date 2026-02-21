//! Helper functions for entry operations.
//!
//! This module contains utility functions for working with filenames and titles.

use crate::workspace::FilenameStyle;

/// Characters that are illegal in filenames on major filesystems (Windows, macOS, Linux).
/// Also forbidden by Chrome's File System Access API on all platforms.
const FS_ILLEGAL_CHARS: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

/// Check if a character is a non-portable control character (U+0000-U+001F, U+007F).
fn is_control_char(c: char) -> bool {
    c <= '\x1F' || c == '\x7F'
}

/// Check if a character is non-portable in filenames.
/// This includes the 9 restricted ASCII symbols and control characters.
fn is_non_portable_char(c: char) -> bool {
    FS_ILLEGAL_CHARS.contains(&c) || is_control_char(c)
}

/// Characters that are not allowed at the start or end of filenames.
/// Chrome's File System Access API forbids `.`, `~`, and whitespace at boundaries.
const BOUNDARY_CHARS: &[char] = &['.', '~', ' ', '\t'];

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
                .filter(|c| !is_non_portable_char(*c))
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

/// Check if a filename contains non-portable characters.
///
/// Returns `Some(reason)` describing the problem, or `None` if the filename is portable.
/// Checks for:
/// - Any of the 9 restricted ASCII symbols (`/ \ : * ? " < > |`) and control chars
/// - Starting or ending with `.`, `~`, or whitespace
///
/// The `filename` argument should be just the filename (no directory components).
pub fn has_non_portable_chars(filename: &str) -> Option<String> {
    // Strip .md extension for checking (if present), since boundary rules apply to the stem
    let stem = filename.strip_suffix(".md").unwrap_or(filename);

    // Check for non-portable characters anywhere
    for c in stem.chars() {
        if FS_ILLEGAL_CHARS.contains(&c) {
            return Some(format!("contains '{}'", c));
        }
        if is_control_char(c) {
            return Some(format!("contains control character U+{:04X}", c as u32));
        }
    }

    // Check boundary characters at start of stem
    if let Some(first) = stem.chars().next()
        && BOUNDARY_CHARS.contains(&first)
    {
        return Some(format!("starts with '{}'", first));
    }

    // Check boundary characters at end of stem
    if let Some(last) = stem.chars().last()
        && BOUNDARY_CHARS.contains(&last)
    {
        return Some(format!("ends with '{}'", last));
    }

    None
}

/// Sanitize a filename by removing non-portable characters.
///
/// - Strips the 9 restricted ASCII symbols and control characters
/// - Trims `.`, `~`, and whitespace from start and end of the stem
/// - Preserves the `.md` extension if present
/// - Returns `"Untitled.md"` if nothing remains
pub fn sanitize_filename(filename: &str) -> String {
    let (stem, ext) = if let Some(s) = filename.strip_suffix(".md") {
        (s, ".md")
    } else {
        (filename, "")
    };

    // Remove non-portable characters
    let cleaned: String = stem.chars().filter(|c| !is_non_portable_char(*c)).collect();

    // Trim boundary characters from start and end
    let trimmed = cleaned
        .trim_start_matches(|c: char| BOUNDARY_CHARS.contains(&c))
        .trim_end_matches(|c: char| BOUNDARY_CHARS.contains(&c));

    if trimmed.is_empty() {
        format!("Untitled{}", ext)
    } else {
        format!("{}{}", trimmed, ext)
    }
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
    fn test_has_non_portable_chars() {
        // Clean filenames
        assert_eq!(has_non_portable_chars("my-note.md"), None);
        assert_eq!(has_non_portable_chars("Hello World.md"), None);
        assert_eq!(has_non_portable_chars("café notes.md"), None);
        assert_eq!(has_non_portable_chars("README.md"), None);

        // Restricted ASCII symbols
        assert!(has_non_portable_chars("what?.md").is_some());
        assert!(has_non_portable_chars("he said \"hello\".md").is_some());
        assert!(has_non_portable_chars("file:name.md").is_some());
        assert!(has_non_portable_chars("a*b.md").is_some());
        assert!(has_non_portable_chars("a|b.md").is_some());
        assert!(has_non_portable_chars("a<b>.md").is_some());

        // Boundary characters
        assert!(has_non_portable_chars(".hidden.md").is_some());
        assert!(has_non_portable_chars("~temp.md").is_some());
        assert!(has_non_portable_chars(" leading-space.md").is_some());
        assert!(has_non_portable_chars("trailing-space .md").is_some());

        // Control characters
        assert!(has_non_portable_chars("file\x00name.md").is_some());
        assert!(has_non_portable_chars("file\x1Fname.md").is_some());
        assert!(has_non_portable_chars("file\x7Fname.md").is_some());
    }

    #[test]
    fn test_sanitize_filename() {
        // Remove restricted chars
        assert_eq!(sanitize_filename("what?.md"), "what.md");
        assert_eq!(
            sanitize_filename("he said \"hello\".md"),
            "he said hello.md"
        );
        assert_eq!(sanitize_filename("a:b:c.md"), "abc.md");

        // Trim boundary chars
        assert_eq!(sanitize_filename(".hidden.md"), "hidden.md");
        assert_eq!(sanitize_filename("~temp.md"), "temp.md");
        assert_eq!(sanitize_filename(" leading.md"), "leading.md");
        assert_eq!(sanitize_filename("trailing .md"), "trailing.md");
        assert_eq!(sanitize_filename("...dots...md"), "dots.md"); // dots trimmed from stem boundaries

        // Fallback
        assert_eq!(sanitize_filename("???.md"), "Untitled.md");
        assert_eq!(sanitize_filename("*"), "Untitled");

        // Preserve extension
        assert_eq!(sanitize_filename("good-name.md"), "good-name.md");
        assert_eq!(sanitize_filename("no-ext"), "no-ext");
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
