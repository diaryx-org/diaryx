//! Date parsing and formatting utilities.
//!
//! This module provides natural language date parsing via chrono-english and
//! helpers for serializing timestamps with the local timezone offset.

use chrono::{Local, NaiveDate, SecondsFormat, TimeZone};
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

/// Format the current local timestamp as RFC 3339 with an explicit offset.
pub fn current_local_timestamp_rfc3339() -> String {
    Local::now().to_rfc3339_opts(SecondsFormat::Secs, false)
}

/// Format an epoch-millis timestamp as local RFC 3339 with an explicit offset.
pub fn timestamp_millis_to_local_rfc3339(timestamp_millis: i64) -> Option<String> {
    Local
        .timestamp_millis_opt(timestamp_millis)
        .single()
        .map(|dt| dt.to_rfc3339_opts(SecondsFormat::Millis, false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Duration};

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

    #[test]
    fn test_current_local_timestamp_rfc3339_uses_explicit_offset() {
        let timestamp = current_local_timestamp_rfc3339();
        let parsed = DateTime::parse_from_rfc3339(&timestamp).unwrap();
        assert_eq!(
            parsed.to_rfc3339_opts(SecondsFormat::Secs, false),
            timestamp
        );
        assert!(!timestamp.ends_with('Z'));
    }

    #[test]
    fn test_timestamp_millis_to_local_rfc3339_round_trips_epoch() {
        let timestamp = timestamp_millis_to_local_rfc3339(1_700_000_000_000).unwrap();
        let parsed = DateTime::parse_from_rfc3339(&timestamp).unwrap();
        assert_eq!(parsed.timestamp_millis(), 1_700_000_000_000);
        assert!(!timestamp.ends_with('Z'));
    }
}
