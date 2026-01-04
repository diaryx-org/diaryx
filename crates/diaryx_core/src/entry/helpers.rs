//! Helper functions for entry operations.
//!
//! This module contains utility functions for working with filenames and titles.

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
}
