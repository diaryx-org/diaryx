//! Creation-time template engine for creating entries with pre-defined structures.
//!
//! Supports simple variable substitution using `{{variable}}` syntax.
//! Variables can include format specifiers for dates: `{{date:%Y-%m-%d}}`
//!
//! This module handles **creation-time** templates that run once when an entry is
//! created. For **render-time** body templating (Handlebars `{{#each}}`, `{{#if}}`,
//! custom helpers) that runs on every view/publish, see [`render`](crate::render).

use chrono::{Local, NaiveDate};
use indexmap::IndexMap;
use serde_yaml::Value;

/// Available template variables and their descriptions.
pub const TEMPLATE_VARIABLES: &[(&str, &str)] = &[
    ("title", "The entry title"),
    ("filename", "The filename without extension"),
    (
        "date",
        "Current date (default: %Y-%m-%d). Use {{date:%B %d, %Y}} for custom format",
    ),
    (
        "time",
        "Current time (default: %H:%M). Use {{time:%H:%M:%S}} for custom format",
    ),
    (
        "datetime",
        "Current datetime (default: %Y-%m-%dT%H:%M:%S). Use {{datetime:FORMAT}} for custom",
    ),
    (
        "timestamp",
        "ISO 8601 timestamp with timezone (for created/updated)",
    ),
    ("year", "Current year (4 digits)"),
    ("month", "Current month (2 digits)"),
    ("month_name", "Current month name (e.g., January)"),
    ("day", "Current day (2 digits)"),
    ("weekday", "Current weekday name (e.g., Monday)"),
];

/// Built-in default template for notes.
pub const DEFAULT_NOTE_TEMPLATE: &str = r#"---
title: "{{title}}"
created: {{timestamp}}
---

# {{title}}

"#;

/// A parsed template with frontmatter and body.
#[derive(Debug, Clone)]
pub struct Template {
    /// Template name (derived from filename).
    pub name: String,
    /// Raw template content (before variable substitution).
    pub raw_content: String,
}

impl Template {
    /// Create a new template from raw content.
    pub fn new(name: impl Into<String>, raw_content: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            raw_content: raw_content.into(),
        }
    }

    /// Get the built-in note template.
    pub fn builtin_note() -> Self {
        Self::new("note", DEFAULT_NOTE_TEMPLATE)
    }

    /// Render the template with the given context.
    pub fn render(&self, context: &TemplateContext) -> String {
        substitute_variables(&self.raw_content, context)
    }

    /// Render and parse into frontmatter and body.
    pub fn render_parsed(
        &self,
        context: &TemplateContext,
    ) -> Result<(IndexMap<String, Value>, String), String> {
        let rendered = self.render(context);
        parse_rendered_template(&rendered)
    }
}

/// Context for template variable substitution.
#[derive(Debug, Clone, Default)]
pub struct TemplateContext {
    /// Title for the entry.
    pub title: Option<String>,
    /// Filename (without extension).
    pub filename: Option<String>,
    /// Date to use (defaults to today).
    pub date: Option<NaiveDate>,
    /// Part of reference (for hierarchical entries).
    pub part_of: Option<String>,
    /// Custom variables.
    pub custom: IndexMap<String, String>,
}

impl TemplateContext {
    /// Create a new empty context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the title.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set the filename.
    pub fn with_filename(mut self, filename: impl Into<String>) -> Self {
        self.filename = Some(filename.into());
        self
    }

    /// Set the date.
    pub fn with_date(mut self, date: NaiveDate) -> Self {
        self.date = Some(date);
        self
    }

    /// Set the part_of reference.
    pub fn with_part_of(mut self, part_of: impl Into<String>) -> Self {
        self.part_of = Some(part_of.into());
        self
    }

    /// Add a custom variable.
    pub fn with_custom(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.custom.insert(key.into(), value.into());
        self
    }

    /// Get the effective date (provided or today).
    pub fn effective_date(&self) -> NaiveDate {
        self.date.unwrap_or_else(|| Local::now().date_naive())
    }

    /// Get the effective title (provided, filename, or "Untitled").
    pub fn effective_title(&self) -> String {
        self.title
            .clone()
            .or_else(|| self.filename.clone())
            .unwrap_or_else(|| "Untitled".to_string())
    }
}

/// Substitute template variables in a string.
pub fn substitute_variables(content: &str, context: &TemplateContext) -> String {
    let mut result = content.to_string();
    let now = Local::now();
    let date = context.effective_date();

    // Process variables with format specifiers first (e.g., {{date:%Y-%m-%d}})
    result = substitute_formatted_variables(&result, "date", |fmt| date.format(fmt).to_string());
    result = substitute_formatted_variables(&result, "time", |fmt| now.format(fmt).to_string());
    result = substitute_formatted_variables(&result, "datetime", |fmt| now.format(fmt).to_string());

    // Simple variable substitutions
    let replacements: Vec<(&str, String)> = vec![
        ("title", context.effective_title()),
        ("filename", context.filename.clone().unwrap_or_default()),
        ("date", date.format("%Y-%m-%d").to_string()),
        ("time", now.format("%H:%M").to_string()),
        ("datetime", now.format("%Y-%m-%dT%H:%M:%S").to_string()),
        ("timestamp", now.format("%Y-%m-%dT%H:%M:%S%:z").to_string()),
        ("year", date.format("%Y").to_string()),
        ("month", date.format("%m").to_string()),
        ("month_name", date.format("%B").to_string()),
        ("day", date.format("%d").to_string()),
        ("weekday", date.format("%A").to_string()),
        ("part_of", context.part_of.clone().unwrap_or_default()),
    ];

    for (var, value) in replacements {
        let pattern = format!("{{{{{}}}}}", var);
        result = result.replace(&pattern, &value);
    }

    // Custom variables
    for (key, value) in &context.custom {
        let pattern = format!("{{{{{}}}}}", key);
        result = result.replace(&pattern, value);
    }

    result
}

/// Substitute variables with format specifiers like `{{var:FORMAT}}`.
pub fn substitute_formatted_variables<F>(content: &str, var_name: &str, formatter: F) -> String
where
    F: Fn(&str) -> String,
{
    let mut result = content.to_string();
    let prefix = format!("{{{{{}:", var_name);

    while let Some(start) = result.find(&prefix) {
        let rest = &result[start + prefix.len()..];
        if let Some(end) = rest.find("}}") {
            let format_str = &rest[..end];
            let full_pattern = format!("{{{{{}:{}}}}}", var_name, format_str);
            let replacement = formatter(format_str);
            result = result.replace(&full_pattern, &replacement);
        } else {
            break;
        }
    }

    result
}

/// Parse rendered template content into frontmatter and body.
pub fn parse_rendered_template(content: &str) -> Result<(IndexMap<String, Value>, String), String> {
    // Check if content starts with frontmatter delimiter
    if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
        // No frontmatter, entire content is body
        return Ok((IndexMap::new(), content.to_string()));
    }

    // Find the closing delimiter
    let rest = &content[4..]; // Skip first "---\n"
    let end_idx = rest.find("\n---\n").or_else(|| rest.find("\n---\r\n"));

    match end_idx {
        Some(idx) => {
            let frontmatter_str = &rest[..idx];
            let body = &rest[idx + 5..]; // Skip "\n---\n"

            let frontmatter: IndexMap<String, Value> = serde_yaml::from_str(frontmatter_str)
                .map_err(|e| format!("Failed to parse frontmatter YAML: {e}"))?;
            Ok((frontmatter, body.to_string()))
        }
        None => {
            // Malformed frontmatter (no closing delimiter) - treat as no frontmatter
            Ok((IndexMap::new(), content.to_string()))
        }
    }
}

/// Information about a template.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TemplateInfo {
    /// Template name.
    pub name: String,
    /// Source of the template ("builtin", "workspace", or "user").
    pub source: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_variable_substitution() {
        let template = Template::new("test", "Hello {{title}}!");
        let context = TemplateContext::new().with_title("World");
        let result = template.render(&context);
        assert_eq!(result, "Hello World!");
    }

    #[test]
    fn test_date_variables() {
        let template = Template::new("test", "Date: {{date}}, Year: {{year}}, Month: {{month}}");
        let date = NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();
        let context = TemplateContext::new().with_date(date);
        let result = template.render(&context);
        assert_eq!(result, "Date: 2024-06-15, Year: 2024, Month: 06");
    }

    #[test]
    fn test_formatted_date_variable() {
        let template = Template::new("test", "{{date:%B %d, %Y}}");
        let date = NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();
        let context = TemplateContext::new().with_date(date);
        let result = template.render(&context);
        assert_eq!(result, "June 15, 2024");
    }

    #[test]
    fn test_custom_variables() {
        let template = Template::new("test", "Mood: {{mood}}, Weather: {{weather}}");
        let context = TemplateContext::new()
            .with_custom("mood", "happy")
            .with_custom("weather", "sunny");
        let result = template.render(&context);
        assert_eq!(result, "Mood: happy, Weather: sunny");
    }

    #[test]
    fn test_builtin_note_template() {
        let template = Template::builtin_note();
        let context = TemplateContext::new().with_title("My Note");
        let result = template.render(&context);

        assert!(result.contains("title: \"My Note\""));
        assert!(result.contains("# My Note"));
        assert!(result.contains("created:"));
    }

    #[test]
    fn test_render_parsed() {
        let template = Template::new("test", "---\ntitle: \"{{title}}\"\n---\n\n# {{title}}\n");
        let context = TemplateContext::new().with_title("Test");
        let (frontmatter, body) = template.render_parsed(&context).unwrap();

        assert_eq!(frontmatter.get("title").unwrap().as_str().unwrap(), "Test");
        assert_eq!(body.trim(), "# Test");
    }

    #[test]
    fn test_effective_title_fallback() {
        // With title
        let ctx = TemplateContext::new().with_title("My Title");
        assert_eq!(ctx.effective_title(), "My Title");

        // Without title, with filename
        let ctx = TemplateContext::new().with_filename("my-file");
        assert_eq!(ctx.effective_title(), "my-file");

        // Without title or filename
        let ctx = TemplateContext::new();
        assert_eq!(ctx.effective_title(), "Untitled");
    }

    #[test]
    fn test_part_of_empty_when_not_set() {
        let template = Template::new("test", "part_of: {{part_of}}");
        let context = TemplateContext::new();
        let result = template.render(&context);
        assert_eq!(result, "part_of: ");
    }

    #[test]
    fn test_timestamp_format() {
        let template = Template::new("test", "{{timestamp}}");
        let context = TemplateContext::new();
        let result = template.render(&context);

        // Should match ISO 8601 with timezone like "2024-06-15T10:30:00-07:00"
        assert!(result.contains("T"));
        assert!(result.contains(":"));
        // Should have timezone offset
        assert!(result.contains("+") || result.contains("-"));
    }
}
