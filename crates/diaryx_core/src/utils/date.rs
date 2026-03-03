//! Date parsing utilities.
//!
//! This module provides natural language date parsing via chrono-english.

use chrono::{Local, NaiveDate};
use chrono_english::{Dialect, parse_date_string};

use crate::error::{DiaryxError, Result};

/// Parse a date string into a NaiveDate
/// Supports natural language dates via chrono-english:
/// - "today", "yesterday", "tomorrow"
/// - "3 days ago", "in 5 days"
/// - "last friday", "next monday", "this wednesday"
/// - "last week", "last month"
/// - "YYYY-MM-DD" format
pub fn parse_date(date_str: &str) -> Result<NaiveDate> {
    let now = Local::now();

    // First try parsing as YYYY-MM-DD for exact dates
    if let Ok(date) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
        return Ok(date);
    }

    // Use chrono-english for natural language parsing
    parse_date_string(date_str, now, Dialect::Us)
        .map(|dt| dt.date_naive())
        .map_err(|_| DiaryxError::InvalidDateFormat(date_str.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_parse_date_iso_format() {
        let date = parse_date("2024-01-15").unwrap();
        assert_eq!(date, NaiveDate::from_ymd_opt(2024, 1, 15).unwrap());
    }

    #[test]
    fn test_parse_date_today() {
        let date = parse_date("today").unwrap();
        assert_eq!(date, Local::now().date_naive());
    }

    #[test]
    fn test_parse_date_yesterday() {
        let date = parse_date("yesterday").unwrap();
        assert_eq!(date, Local::now().date_naive() - Duration::days(1));
    }

    #[test]
    fn test_parse_date_tomorrow() {
        let date = parse_date("tomorrow").unwrap();
        assert_eq!(date, Local::now().date_naive() + Duration::days(1));
    }

    #[test]
    fn test_parse_date_days_ago() {
        let date = parse_date("3 days ago").unwrap();
        assert_eq!(date, Local::now().date_naive() - Duration::days(3));
    }

    #[test]
    fn test_parse_date_invalid() {
        assert!(parse_date("not a date").is_err());
    }
}
