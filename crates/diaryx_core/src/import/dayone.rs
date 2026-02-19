//! Day One journal import — parse `Journal.json` files into [`ImportedEntry`] values.
//!
//! Day One exports journals as a single JSON file containing a `metadata` object
//! and an `entries` array. Each entry has a `text` field with escaped markdown,
//! a `creationDate`, and optional `location`, `weather`, `tags`, and media arrays.
//!
//! This parser is a pure function (no I/O) — callers provide the raw bytes.

use indexmap::IndexMap;
use serde::Deserialize;
use serde_yaml::Value;

use super::{ImportedAttachment, ImportedEntry};

/// Top-level Day One JSON structure.
#[derive(Deserialize)]
struct DayOneJournal {
    entries: Vec<DayOneEntry>,
}

/// A single Day One journal entry.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DayOneEntry {
    uuid: Option<String>,
    text: Option<String>,
    creation_date: Option<String>,
    starred: Option<bool>,
    tags: Option<Vec<String>>,
    location: Option<DayOneLocation>,
    weather: Option<DayOneWeather>,
    photos: Option<Vec<DayOneMedia>>,
    videos: Option<Vec<DayOneMedia>>,
    audios: Option<Vec<DayOneMedia>>,
}

/// Location metadata from Day One.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DayOneLocation {
    locality_name: Option<String>,
    administrative_area: Option<String>,
    country: Option<String>,
}

/// Weather metadata from Day One.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DayOneWeather {
    conditions_description: Option<String>,
    temperature_celsius: Option<f64>,
}

/// A media reference (photo, video, or audio) in a Day One entry.
#[derive(Deserialize)]
struct DayOneMedia {
    identifier: Option<String>,
    #[serde(rename = "type")]
    media_type: Option<String>,
}

/// Parse a Day One `Journal.json` file into [`ImportedEntry`] values.
///
/// Returns one `Result` per entry so callers can skip unparseable entries
/// while still importing the rest.
pub fn parse_dayone(bytes: &[u8]) -> Vec<Result<ImportedEntry, String>> {
    let journal: DayOneJournal = match serde_json::from_slice(bytes) {
        Ok(j) => j,
        Err(e) => return vec![Err(format!("Failed to parse Journal.json: {e}"))],
    };

    journal.entries.into_iter().map(convert_entry).collect()
}

/// Convert a single deserialized Day One entry into an [`ImportedEntry`].
fn convert_entry(entry: DayOneEntry) -> Result<ImportedEntry, String> {
    let raw_text = entry.text.unwrap_or_default();
    let unescaped = unescape_dayone_markdown(&raw_text);

    let (title, body) = extract_title_and_body(&unescaped);

    let date = entry
        .creation_date
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc));

    let mut metadata = IndexMap::new();

    // UUID for deduplication/provenance
    if let Some(ref uuid) = entry.uuid {
        metadata.insert("uuid".to_string(), Value::String(uuid.clone()));
    }

    // Tags (only if non-empty)
    if let Some(ref tags) = entry.tags {
        if !tags.is_empty() {
            let tag_values: Vec<Value> = tags.iter().map(|t| Value::String(t.clone())).collect();
            metadata.insert("tags".to_string(), Value::Sequence(tag_values));
        }
    }

    // Starred
    if entry.starred == Some(true) {
        metadata.insert("starred".to_string(), Value::Bool(true));
    }

    // Location
    if let Some(ref loc) = entry.location {
        if let Some(loc_str) = format_location(loc) {
            metadata.insert("location".to_string(), Value::String(loc_str));
        }
    }

    // Weather
    if let Some(ref weather) = entry.weather {
        if let Some(weather_str) = format_weather(weather) {
            metadata.insert("weather".to_string(), Value::String(weather_str));
        }
    }

    // Media counts (photos, videos, audios) — record identifiers for future use
    let mut attachments = Vec::new();
    for media_list in [&entry.photos, &entry.videos, &entry.audios] {
        if let Some(items) = media_list {
            for item in items {
                let id = item.identifier.clone().unwrap_or_default();
                let ext = item.media_type.clone().unwrap_or_else(|| "bin".to_string());
                let filename = if id.is_empty() {
                    format!("media.{ext}")
                } else {
                    format!("{id}.{ext}")
                };
                attachments.push(ImportedAttachment {
                    filename,
                    content_type: format!("application/octet-stream"),
                    data: Vec::new(), // Day One JSON doesn't embed binary data
                });
            }
        }
    }

    Ok(ImportedEntry {
        title,
        date,
        body,
        metadata,
        attachments,
    })
}

/// Unescape Day One's markdown escape sequences.
///
/// Day One escapes characters that have special meaning in markdown:
/// `\.` `\!` `\-` `\(` `\)` `\+` `\*` `\[` `\]` `\#`
fn unescape_dayone_markdown(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(&next) = chars.peek() {
                match next {
                    '.' | '!' | '-' | '(' | ')' | '+' | '*' | '[' | ']' | '#' => {
                        result.push(next);
                        chars.next();
                    }
                    _ => {
                        result.push(ch);
                    }
                }
            } else {
                result.push(ch);
            }
        } else {
            result.push(ch);
        }
    }

    result
}

/// Extract title from the first `# heading` line and return (title, remaining_body).
///
/// If no heading is found, falls back to "Untitled".
fn extract_title_and_body(text: &str) -> (String, String) {
    // Look for a `# ` heading at the start of the text
    let trimmed = text.trim_start();

    if let Some(rest) = trimmed.strip_prefix("# ") {
        // Find end of heading line
        if let Some(newline_pos) = rest.find('\n') {
            let title = rest[..newline_pos].trim().to_string();
            let body = rest[newline_pos + 1..].to_string();
            let title = if title.is_empty() {
                "Untitled".to_string()
            } else {
                title
            };
            (title, body)
        } else {
            // Entire text is the heading
            let title = rest.trim().to_string();
            let title = if title.is_empty() {
                "Untitled".to_string()
            } else {
                title
            };
            (title, String::new())
        }
    } else {
        ("Untitled".to_string(), text.to_string())
    }
}

/// Format location as "City, State, Country", omitting missing parts.
fn format_location(loc: &DayOneLocation) -> Option<String> {
    let parts: Vec<&str> = [
        loc.locality_name.as_deref(),
        loc.administrative_area.as_deref(),
        loc.country.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect();

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

/// Format weather as "Conditions, Temp°C".
fn format_weather(weather: &DayOneWeather) -> Option<String> {
    match (
        weather.conditions_description.as_deref(),
        weather.temperature_celsius,
    ) {
        (Some(cond), Some(temp)) => {
            // Format temperature: show integer if whole, otherwise one decimal
            let temp_str = if temp.fract() == 0.0 {
                format!("{:.0}", temp)
            } else {
                format!("{:.1}", temp)
            };
            Some(format!("{cond}, {temp_str}\u{00B0}C"))
        }
        (Some(cond), None) => Some(cond.to_string()),
        (None, Some(temp)) => {
            let temp_str = if temp.fract() == 0.0 {
                format!("{:.0}", temp)
            } else {
                format!("{:.1}", temp)
            };
            Some(format!("{temp_str}\u{00B0}C"))
        }
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unescape_dayone_markdown() {
        assert_eq!(unescape_dayone_markdown(r"Hello\."), "Hello.");
        assert_eq!(unescape_dayone_markdown(r"test\!"), "test!");
        assert_eq!(unescape_dayone_markdown(r"a\-b"), "a-b");
        assert_eq!(unescape_dayone_markdown(r"\(foo\)"), "(foo)");
        assert_eq!(unescape_dayone_markdown(r"1\+2"), "1+2");
        assert_eq!(unescape_dayone_markdown(r"\*bold\*"), "*bold*");
        assert_eq!(unescape_dayone_markdown(r"\[link\]"), "[link]");
        assert_eq!(unescape_dayone_markdown(r"\# heading"), "# heading");
        // Backslash before non-special char is preserved
        assert_eq!(unescape_dayone_markdown(r"path\to"), r"path\to");
        // Trailing backslash is preserved
        assert_eq!(unescape_dayone_markdown("end\\"), "end\\");
        // No escapes
        assert_eq!(unescape_dayone_markdown("plain text"), "plain text");
    }

    #[test]
    fn test_extract_title_and_body() {
        let (title, body) = extract_title_and_body("# My Title\nBody here.");
        assert_eq!(title, "My Title");
        assert_eq!(body, "Body here.");

        let (title, body) = extract_title_and_body("# Just a title");
        assert_eq!(title, "Just a title");
        assert_eq!(body, "");

        let (title, body) = extract_title_and_body("No heading here.");
        assert_eq!(title, "Untitled");
        assert_eq!(body, "No heading here.");

        let (title, body) = extract_title_and_body("# \nBody after empty heading.");
        assert_eq!(title, "Untitled");
        assert_eq!(body, "Body after empty heading.");
    }

    #[test]
    fn test_format_location() {
        let loc = DayOneLocation {
            locality_name: Some("Vidor".to_string()),
            administrative_area: Some("TX".to_string()),
            country: Some("United States".to_string()),
        };
        assert_eq!(
            format_location(&loc),
            Some("Vidor, TX, United States".to_string())
        );

        let partial = DayOneLocation {
            locality_name: Some("Austin".to_string()),
            administrative_area: None,
            country: Some("United States".to_string()),
        };
        assert_eq!(
            format_location(&partial),
            Some("Austin, United States".to_string())
        );

        let empty = DayOneLocation {
            locality_name: None,
            administrative_area: None,
            country: None,
        };
        assert_eq!(format_location(&empty), None);
    }

    #[test]
    fn test_format_weather() {
        let w = DayOneWeather {
            conditions_description: Some("Cloudy".to_string()),
            temperature_celsius: Some(23.0),
        };
        assert_eq!(format_weather(&w), Some("Cloudy, 23°C".to_string()));

        let fractional = DayOneWeather {
            conditions_description: Some("Clear".to_string()),
            temperature_celsius: Some(19.8),
        };
        assert_eq!(
            format_weather(&fractional),
            Some("Clear, 19.8°C".to_string())
        );

        let cond_only = DayOneWeather {
            conditions_description: Some("Rainy".to_string()),
            temperature_celsius: None,
        };
        assert_eq!(format_weather(&cond_only), Some("Rainy".to_string()));

        let empty = DayOneWeather {
            conditions_description: None,
            temperature_celsius: None,
        };
        assert_eq!(format_weather(&empty), None);
    }

    #[test]
    fn test_parse_minimal_entry() {
        let json = r##"{
            "metadata": { "version": "1.0" },
            "entries": [{
                "uuid": "ABC123",
                "text": "# Hello World\nThis is a test.",
                "creationDate": "2020-09-24T01:36:35Z",
                "starred": false,
                "tags": [],
                "isPinned": false
            }]
        }"##;

        let results = parse_dayone(json.as_bytes());
        assert_eq!(results.len(), 1);

        let entry = results.into_iter().next().unwrap().unwrap();
        assert_eq!(entry.title, "Hello World");
        assert_eq!(entry.body, "This is a test.");
        assert!(entry.date.is_some());
        assert_eq!(
            entry.metadata.get("uuid").unwrap(),
            &Value::String("ABC123".to_string())
        );
    }

    #[test]
    fn test_parse_entry_with_metadata() {
        let json = r##"{
            "metadata": { "version": "1.0" },
            "entries": [{
                "uuid": "DEF456",
                "text": "# Tagged Entry\nWith location and weather.",
                "creationDate": "2020-04-27T03:04:51Z",
                "starred": true,
                "tags": ["reflection", "personal"],
                "location": {
                    "localityName": "Austin",
                    "administrativeArea": "TX",
                    "country": "United States"
                },
                "weather": {
                    "conditionsDescription": "Sunny",
                    "temperatureCelsius": 30
                }
            }]
        }"##;

        let results = parse_dayone(json.as_bytes());
        let entry = results.into_iter().next().unwrap().unwrap();

        assert_eq!(entry.title, "Tagged Entry");
        assert_eq!(entry.metadata.get("starred").unwrap(), &Value::Bool(true));

        let tags = entry.metadata.get("tags").unwrap();
        if let Value::Sequence(seq) = tags {
            assert_eq!(seq.len(), 2);
        } else {
            panic!("tags should be a sequence");
        }

        assert_eq!(
            entry.metadata.get("location").unwrap(),
            &Value::String("Austin, TX, United States".to_string())
        );
        assert_eq!(
            entry.metadata.get("weather").unwrap(),
            &Value::String("Sunny, 30°C".to_string())
        );
    }

    #[test]
    fn test_parse_entry_with_escapes() {
        let json = r##"{
            "metadata": { "version": "1.0" },
            "entries": [{
                "text": "# Hello\\.\nBody with escapes\\. And \\!exclamation\\!",
                "creationDate": "2020-01-01T00:00:00Z"
            }]
        }"##;

        let results = parse_dayone(json.as_bytes());
        let entry = results.into_iter().next().unwrap().unwrap();

        assert_eq!(entry.title, "Hello.");
        assert_eq!(entry.body, "Body with escapes. And !exclamation!");
    }

    #[test]
    fn test_parse_invalid_json() {
        let results = parse_dayone(b"not json");
        assert_eq!(results.len(), 1);
        assert!(results[0].is_err());
    }

    #[test]
    fn test_parse_no_heading() {
        let json = r##"{
            "metadata": { "version": "1.0" },
            "entries": [{
                "text": "Just some text without a heading.",
                "creationDate": "2020-01-01T00:00:00Z"
            }]
        }"##;

        let results = parse_dayone(json.as_bytes());
        let entry = results.into_iter().next().unwrap().unwrap();
        assert_eq!(entry.title, "Untitled");
        assert_eq!(entry.body, "Just some text without a heading.");
    }
}
