//! Shared daily-entry domain logic for Diaryx daily plugins.

use chrono::{Duration, Local, NaiveDate};
use chrono_english::{Dialect, parse_date_string};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DailyError {
    #[error("Invalid date format: {0}")]
    InvalidDate(String),
    #[error("Path is not a daily entry path")]
    NotDailyPath,
}

pub type Result<T> = std::result::Result<T, DailyError>;

/// Plugin-owned daily configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DailyPluginConfig {
    /// Optional folder prefix where daily hierarchy is stored.
    /// If absent, callers should use [`DailyPluginConfig::effective_entry_folder`].
    #[serde(default)]
    pub entry_folder: Option<String>,
    /// Optional template source. This can be inline template text or a path/link reference.
    #[serde(default)]
    pub entry_template: Option<String>,
    /// Whether one-time migration from legacy workspace fields has completed.
    #[serde(default)]
    pub migrated_legacy_config: bool,
}

impl DailyPluginConfig {
    /// Effective folder for daily entries.
    pub fn effective_entry_folder(&self) -> String {
        let normalized = normalize_folder(self.entry_folder.as_deref());
        if normalized.is_empty() {
            "Daily".to_string()
        } else {
            normalized
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyPaths {
    pub folder: String,
    pub daily_index: String,
    pub year_index: String,
    pub month_index: String,
    pub entry: String,
}

/// Parse an optional date string. `None` defaults to "today".
pub fn parse_date_input(date: Option<&str>) -> Result<NaiveDate> {
    let input = date.unwrap_or("today");

    if let Ok(parsed) = NaiveDate::parse_from_str(input, "%Y-%m-%d") {
        return Ok(parsed);
    }

    parse_date_string(input, Local::now(), Dialect::Us)
        .map(|dt| dt.date_naive())
        .map_err(|_| DailyError::InvalidDate(input.to_string()))
}

/// Convert a date to `YYYY/MM/YYYY-MM-DD.md`.
pub fn entry_relative_path(date: NaiveDate) -> String {
    format!(
        "{}/{}/{}.md",
        date.format("%Y"),
        date.format("%m"),
        date.format("%Y-%m-%d")
    )
}

/// Return true if the path matches `.../YYYY/MM/YYYY-MM-DD.md` and segments align.
pub fn is_daily_entry_path(path: &str) -> bool {
    path_to_date(path).is_ok()
}

/// Parse a `YYYY-MM-DD` date from a daily entry path.
pub fn path_to_date(path: &str) -> Result<NaiveDate> {
    let path = path.replace('\\', "/");
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() < 3 {
        return Err(DailyError::NotDailyPath);
    }

    let filename = parts[parts.len() - 1];
    let month = parts[parts.len() - 2];
    let year = parts[parts.len() - 3];

    if !filename.ends_with(".md") {
        return Err(DailyError::NotDailyPath);
    }

    let stem = filename.trim_end_matches(".md");
    let date = NaiveDate::parse_from_str(stem, "%Y-%m-%d").map_err(|_| DailyError::NotDailyPath)?;

    if year != date.format("%Y").to_string() || month != date.format("%m").to_string() {
        return Err(DailyError::NotDailyPath);
    }

    Ok(date)
}

/// Get adjacent daily entry path using the same base prefix.
pub fn adjacent_daily_entry_path(path: &str, direction: DailyDirection) -> Result<String> {
    let date = path_to_date(path)?;
    let offset = match direction {
        DailyDirection::Prev => -1,
        DailyDirection::Next => 1,
    };
    let target_date = date
        .checked_add_signed(Duration::days(offset))
        .ok_or(DailyError::NotDailyPath)?;

    let normalized = path.replace('\\', "/");
    let parts: Vec<&str> = normalized.split('/').filter(|s| !s.is_empty()).collect();
    let prefix = if parts.len() > 3 {
        parts[..parts.len() - 3].join("/")
    } else {
        String::new()
    };

    let rel = entry_relative_path(target_date);
    if prefix.is_empty() {
        Ok(rel)
    } else {
        Ok(format!("{prefix}/{rel}"))
    }
}

pub fn year_index_filename(date: NaiveDate) -> String {
    format!("{}_index.md", date.format("%Y"))
}

pub fn month_index_filename(date: NaiveDate) -> String {
    format!(
        "{}_{}.md",
        date.format("%Y"),
        date.format("%B").to_string().to_lowercase()
    )
}

pub fn normalize_folder(folder: Option<&str>) -> String {
    folder.unwrap_or_default().trim_matches('/').to_string()
}

pub fn scoped_path(prefix: &str, rel: &str) -> String {
    if prefix.is_empty() {
        rel.to_string()
    } else {
        format!("{prefix}/{rel}")
    }
}

/// Build all canonical daily hierarchy paths for a date.
pub fn paths_for_date(folder: &str, date: NaiveDate) -> DailyPaths {
    let year = date.format("%Y").to_string();
    let month = date.format("%m").to_string();
    let daily_index = scoped_path(folder, "daily_index.md");
    let year_index = scoped_path(folder, &format!("{year}/{}", year_index_filename(date)));
    let month_index = scoped_path(
        folder,
        &format!("{year}/{month}/{}", month_index_filename(date)),
    );
    let entry = scoped_path(folder, &entry_relative_path(date));

    DailyPaths {
        folder: folder.to_string(),
        daily_index,
        year_index,
        month_index,
        entry,
    }
}

pub fn render_template(template: &str, title: &str, date: NaiveDate, part_of: &str) -> String {
    let mut out = template.to_string();
    out = out.replace("{{title}}", title);
    out = out.replace("{{date}}", &date.format("%Y-%m-%d").to_string());
    out = out.replace("{{part_of}}", part_of);
    out = out.replace(
        "{{timestamp}}",
        &Local::now().format("%Y-%m-%dT%H:%M:%S%:z").to_string(),
    );
    out
}

pub fn default_entry_template() -> &'static str {
    "---\ntitle: \"{{title}}\"\ndate: {{date}}\ncreated: {{timestamp}}\npart_of: \"{{part_of}}\"\n---\n\n# {{title}}\n\n"
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DailyDirection {
    Prev,
    Next,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_daily_path() {
        let parsed = path_to_date("Daily/2026/03/2026-03-02.md").unwrap();
        assert_eq!(parsed, NaiveDate::from_ymd_opt(2026, 3, 2).unwrap());
    }

    #[test]
    fn adjacent_preserves_prefix() {
        let next =
            adjacent_daily_entry_path("Journal/Daily/2026/03/2026-03-02.md", DailyDirection::Next)
                .unwrap();
        assert_eq!(next, "Journal/Daily/2026/03/2026-03-03.md");
    }

    #[test]
    fn default_folder_is_daily() {
        let config = DailyPluginConfig::default();
        assert_eq!(config.effective_entry_folder(), "Daily");
    }

    #[test]
    fn builds_daily_paths() {
        let date = NaiveDate::from_ymd_opt(2026, 3, 2).unwrap();
        let paths = paths_for_date("Daily", date);
        assert_eq!(paths.daily_index, "Daily/daily_index.md");
        assert_eq!(paths.year_index, "Daily/2026/2026_index.md");
        assert_eq!(paths.month_index, "Daily/2026/03/2026_march.md");
        assert_eq!(paths.entry, "Daily/2026/03/2026-03-02.md");
    }
}
