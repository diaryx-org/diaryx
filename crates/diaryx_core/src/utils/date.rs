//! Date formatting utilities.
//!
//! Helpers for serializing timestamps with the local timezone offset.
//!
//! Natural language date parsing ("today", "3 days ago", etc.) lives in the
//! `diaryx_daily_extism` plugin.

use chrono::{Local, SecondsFormat, TimeZone};

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
    use chrono::{DateTime, SecondsFormat};

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
