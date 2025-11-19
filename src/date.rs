use chrono::{Local, NaiveDate, Duration};
use std::path::PathBuf;

/// Parse a date string into a NaiveDate
/// Supports: "today", "yesterday", "YYYY-MM-DD"
pub fn parse_date(date_str: &str) -> Result<NaiveDate, DateError> {
    match date_str.to_lowercase().as_str() {
        "today" => Ok(Local::now().date_naive()),
        "yesterday" => Ok(Local::now().date_naive() - Duration::days(1)),
        _ => {
            // Try parsing as YYYY-MM-DD
            NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
                .map_err(|_| DateError::InvalidFormat(date_str.to_string()))
        }
    }
}

/// Generate the file path for a given date
/// Format: {base_dir}/YYYY/MM/YYYY-MM-DD.md
pub fn date_to_path(base_dir: &PathBuf, date: &NaiveDate) -> PathBuf {
    let year = date.format("%Y").to_string();
    let month = date.format("%m").to_string();
    let filename = format!("{}.md", date.format("%Y-%m-%d"));

    base_dir.join(&year).join(&month).join(filename)
}

/// Extract date from a path if it matches the expected format
/// Returns None if path doesn't match YYYY/MM/YYYY-MM-DD.md
pub fn path_to_date(path: &PathBuf) -> Option<NaiveDate> {
    let filename = path.file_stem()?.to_str()?;
    NaiveDate::parse_from_str(filename, "%Y-%m-%d").ok()
}

#[derive(Debug)]
pub enum DateError {
    InvalidFormat(String),
}

impl std::fmt::Display for DateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DateError::InvalidFormat(s) => {
                write!(f, "Invalid date format: '{}'. Use 'today', 'yesterday', or 'YYYY-MM-DD'", s)
            }
        }
    }
}

impl std::error::Error for DateError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_date() {
        // Test YYYY-MM-DD format
        let date = parse_date("2024-01-15").unwrap();
        assert_eq!(date, NaiveDate::from_ymd_opt(2024, 1, 15).unwrap());

        // Test today/yesterday (just ensure they don't panic)
        assert!(parse_date("today").is_ok());
        assert!(parse_date("yesterday").is_ok());

        // Test invalid format
        assert!(parse_date("invalid").is_err());
    }

    #[test]
    fn test_date_to_path() {
        let base = PathBuf::from("/home/user/diary");
        let date = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        let path = date_to_path(&base, &date);

        assert_eq!(path, PathBuf::from("/home/user/diary/2024/01/2024-01-15.md"));
    }

    #[test]
    fn test_path_to_date() {
        let path = PathBuf::from("/home/user/diary/2024/01/2024-01-15.md");
        let date = path_to_date(&path).unwrap();

        assert_eq!(date, NaiveDate::from_ymd_opt(2024, 1, 15).unwrap());

        // Test invalid path
        let invalid_path = PathBuf::from("/home/user/diary/random.md");
        assert!(path_to_date(&invalid_path).is_none());
    }
}
