//! Day One journal import — parse Day One exports into [`ImportedEntry`] values.
//!
//! Day One's "Export as JSON" produces a ZIP file containing a JSON file
//! (named after the journal) plus media directories (`photos/`, `videos/`,
//! `audios/`, `pdfs/`). This module handles both ZIP exports (with full
//! media extraction) and plain JSON files (backward compatible).
//!
//! Use [`parse_dayone_auto`] to auto-detect the format, or call
//! [`parse_dayone`] / [`parse_dayone_zip`] directly.

use std::collections::HashMap;
use std::io::{Cursor, Read};

use crate::yaml_value::YamlValue;
use indexmap::IndexMap;
use serde::Deserialize;

use super::{ImportedAttachment, ImportedEntry};

/// Result of parsing a Day One export.
pub struct DayOneParseResult {
    /// Name of the journal (from ZIP filename, e.g. "Export test").
    /// `None` for plain JSON imports.
    pub journal_name: Option<String>,
    /// One `Result` per entry — callers can skip failures.
    pub entries: Vec<Result<ImportedEntry, String>>,
}

/// Incremental Day One entry stream.
pub struct DayOneEntryStream<'a> {
    /// Name of the journal (from ZIP filename, e.g. "Export test").
    pub journal_name: Option<String>,
    entries: std::vec::IntoIter<DayOneEntry>,
    archive: Option<zip::ZipArchive<Cursor<&'a [u8]>>>,
    media_index: HashMap<String, String>,
}

impl<'a> DayOneEntryStream<'a> {
    /// Get the next imported entry from the source export.
    pub fn next_entry(&mut self) -> Option<Result<ImportedEntry, String>> {
        let entry = self.entries.next()?;
        let result = if let Some(archive) = self.archive.as_mut() {
            let media_index = &self.media_index;
            convert_entry_with_resolver(entry, |item| {
                resolve_media_for_item(archive, media_index, item)
            })
        } else {
            convert_entry_with_resolver(entry, |_| None)
        };
        Some(result)
    }
}

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
    pdf_attachments: Option<Vec<DayOneMedia>>,
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

/// A media reference (photo, video, audio, or PDF) in a Day One entry.
#[derive(Deserialize)]
struct DayOneMedia {
    identifier: Option<String>,
    md5: Option<String>,
    filename: Option<String>,
    #[serde(rename = "type")]
    media_type: Option<String>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse a Day One export, auto-detecting ZIP vs JSON by magic bytes.
///
/// ZIP files start with `PK` (`0x50 0x4B`). Everything else is treated as JSON.
pub fn parse_dayone_auto(bytes: &[u8]) -> DayOneParseResult {
    collect_stream_result(stream_dayone_auto(bytes))
}

/// Parse a Day One `Journal.json` file into [`ImportedEntry`] values.
///
/// Returns one `Result` per entry so callers can skip unparseable entries
/// while still importing the rest. Attachments will have empty `data` since
/// the JSON does not embed binary content — use [`parse_dayone_zip`] for
/// full media extraction.
pub fn parse_dayone(bytes: &[u8]) -> DayOneParseResult {
    collect_stream_result(stream_dayone(bytes))
}

/// Parse a Day One ZIP export into [`ImportedEntry`] values.
///
/// The ZIP should contain one `.json` file at the root (named after the
/// journal) and media files under `photos/`, `videos/`, `audios/`, and/or
/// `pdfs/` directories. Media files are matched to entries by their
/// identifier (filename stem).
pub fn parse_dayone_zip(bytes: &[u8]) -> DayOneParseResult {
    collect_stream_result(stream_dayone_zip(bytes))
}

/// Open a Day One export as an incremental stream, auto-detecting ZIP vs JSON.
pub fn stream_dayone_auto(bytes: &[u8]) -> Result<DayOneEntryStream<'_>, String> {
    if bytes.len() >= 2 && bytes[0] == 0x50 && bytes[1] == 0x4B {
        stream_dayone_zip(bytes)
    } else {
        stream_dayone(bytes)
    }
}

/// Open a Day One JSON export as an incremental stream.
pub fn stream_dayone(bytes: &[u8]) -> Result<DayOneEntryStream<'_>, String> {
    let journal: DayOneJournal =
        serde_json::from_slice(bytes).map_err(|e| format!("Failed to parse Day One JSON: {e}"))?;

    Ok(DayOneEntryStream {
        journal_name: None,
        entries: journal.entries.into_iter(),
        archive: None,
        media_index: HashMap::new(),
    })
}

/// Open a Day One ZIP export as an incremental stream.
pub fn stream_dayone_zip(bytes: &[u8]) -> Result<DayOneEntryStream<'_>, String> {
    let cursor = Cursor::new(bytes);
    let mut archive =
        zip::ZipArchive::new(cursor).map_err(|e| format!("Failed to open ZIP archive: {e}"))?;

    let (journal_json, journal_name) = read_journal_json(&mut archive)?;
    let media_index = build_media_index(&mut archive);
    let journal: DayOneJournal = serde_json::from_slice(&journal_json)
        .map_err(|e| format!("Failed to parse Day One JSON: {e}"))?;

    Ok(DayOneEntryStream {
        journal_name,
        entries: journal.entries.into_iter(),
        archive: Some(archive),
        media_index,
    })
}

fn collect_stream_result(stream: Result<DayOneEntryStream<'_>, String>) -> DayOneParseResult {
    let mut stream = match stream {
        Ok(stream) => stream,
        Err(error) => {
            return DayOneParseResult {
                journal_name: None,
                entries: vec![Err(error)],
            };
        }
    };

    let journal_name = stream.journal_name.clone();
    let mut entries = Vec::new();
    while let Some(entry) = stream.next_entry() {
        entries.push(entry);
    }

    DayOneParseResult {
        journal_name,
        entries,
    }
}

fn read_journal_json(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
) -> Result<(Vec<u8>, Option<String>), String> {
    let mut json_index = None;
    let mut journal_name: Option<String> = None;

    for i in 0..archive.len() {
        let name = match archive.by_index_raw(i) {
            Ok(e) => e.name().to_string(),
            Err(_) => continue,
        };
        // Root-level .json file: no '/' before the name, ends with .json
        if !name.contains('/') && name.ends_with(".json") {
            // Extract journal name from filename (strip .json extension)
            journal_name = Some(name.trim_end_matches(".json").to_string());
            json_index = Some(i);
            break;
        }
    }

    let json_index = json_index
        .ok_or_else(|| "ZIP archive does not contain a .json file at the root level".to_string())?;

    let mut entry = archive
        .by_index(json_index)
        .map_err(|e| format!("Failed to read JSON file from ZIP: {e}"))?;
    let mut buf = Vec::new();
    entry
        .read_to_end(&mut buf)
        .map_err(|e| format!("Failed to read JSON file: {e}"))?;

    Ok((buf, journal_name))
}

fn build_media_index(archive: &mut zip::ZipArchive<Cursor<&[u8]>>) -> HashMap<String, String> {
    let mut media_index: HashMap<String, String> = HashMap::new();

    for i in 0..archive.len() {
        let entry = match archive.by_index_raw(i) {
            Ok(e) => e,
            Err(_) => continue,
        };
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();

        // Only process files in media directories
        let is_media = name.starts_with("photos/")
            || name.starts_with("videos/")
            || name.starts_with("audios/")
            || name.starts_with("pdfs/");
        if !is_media {
            continue;
        }

        // Extract identifier (filename stem)
        let filename = name.rsplit('/').next().unwrap_or(&name);
        let stem = filename
            .rsplit_once('.')
            .map(|(s, _)| s)
            .unwrap_or(filename);

        media_index.insert(normalize_media_key(stem), name);
    }

    media_index
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn convert_entry_with_resolver<F>(
    entry: DayOneEntry,
    mut resolve_media: F,
) -> Result<ImportedEntry, String>
where
    F: FnMut(&DayOneMedia) -> Option<(String, Vec<u8>)>,
{
    let raw_text = entry.text.unwrap_or_default();
    let unescaped = unescape_dayone_markdown(&raw_text);

    let (parsed_title, body) = extract_title_and_body(&unescaped);

    let date = entry
        .creation_date
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc));

    // Derive title: prefer explicit heading, else date + first body line.
    let title = parsed_title.unwrap_or_else(|| derive_title(date, &body));

    let mut metadata = IndexMap::new();

    // UUID for deduplication/provenance
    if let Some(ref uuid) = entry.uuid {
        metadata.insert("uuid".to_string(), YamlValue::String(uuid.clone()));
    }

    // Tags (only if non-empty)
    if let Some(ref tags) = entry.tags {
        if !tags.is_empty() {
            let tag_values: Vec<YamlValue> =
                tags.iter().map(|t| YamlValue::String(t.clone())).collect();
            metadata.insert("tags".to_string(), YamlValue::Sequence(tag_values));
        }
    }

    // Starred
    if entry.starred == Some(true) {
        metadata.insert("starred".to_string(), YamlValue::Bool(true));
    }

    // Location
    if let Some(ref loc) = entry.location {
        if let Some(loc_str) = format_location(loc) {
            metadata.insert("location".to_string(), YamlValue::String(loc_str));
        }
    }

    // Weather
    if let Some(ref weather) = entry.weather {
        if let Some(weather_str) = format_weather(weather) {
            metadata.insert("weather".to_string(), YamlValue::String(weather_str));
        }
    }

    // Media attachments (photos, videos, audios, PDFs)
    let mut attachments = Vec::new();
    let mut attachment_references: Vec<(String, String)> = Vec::new();
    for media_list in [
        &entry.photos,
        &entry.videos,
        &entry.audios,
        &entry.pdf_attachments,
    ] {
        if let Some(items) = media_list {
            for item in items {
                let ext = item.media_type.clone().unwrap_or_else(|| "bin".to_string());

                let (filename, content_type, data) =
                    if let Some((filename, data)) = resolve_media(item) {
                        let actual_ext = filename
                            .rsplit_once('.')
                            .map(|(_, e)| e.to_string())
                            .unwrap_or_else(|| "bin".to_string());
                        (filename, mime_from_extension(&actual_ext).to_string(), data)
                    } else {
                        let fallback_filename = fallback_media_filename(item, &ext);
                        (
                            fallback_filename,
                            mime_from_extension(&ext).to_string(),
                            Vec::new(),
                        )
                    };

                for stem in media_reference_stems(item, &filename) {
                    attachment_references.push((stem, filename.clone()));
                }

                attachments.push(ImportedAttachment {
                    filename,
                    content_type,
                    data,
                });
            }
        }
    }

    // Replace dayone-moment:// and dayone-moment:/ URLs in the body
    // with _attachments/FILENAME references.
    let mut body = body;
    for (stem, filename) in &attachment_references {
        for pattern in [
            format!("dayone-moment://{stem}"),
            format!("dayone-moment:/{stem}"),
        ] {
            body = body.replace(&pattern, &format!("_attachments/{filename}"));
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

fn read_media_attachment(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    media_index: &HashMap<String, String>,
    key: &str,
) -> Option<(String, Vec<u8>)> {
    let archive_path = media_index.get(&normalize_media_key(key))?;
    let filename = archive_path
        .rsplit('/')
        .next()
        .unwrap_or(archive_path)
        .to_string();
    let mut entry = archive.by_name(archive_path).ok()?;
    let mut data = Vec::new();
    entry.read_to_end(&mut data).ok()?;
    Some((filename, data))
}

fn fallback_media_filename(item: &DayOneMedia, ext: &str) -> String {
    if let Some(filename) = item.filename.as_deref().filter(|name| !name.is_empty()) {
        return filename.to_string();
    }
    if let Some(id) = item.identifier.as_deref().filter(|id| !id.is_empty()) {
        return format!("{id}.{ext}");
    }
    if let Some(md5) = item.md5.as_deref().filter(|md5| !md5.is_empty()) {
        return format!("{md5}.{ext}");
    }
    format!("media.{ext}")
}

fn media_reference_stems(item: &DayOneMedia, resolved_filename: &str) -> Vec<String> {
    let mut stems = Vec::new();

    for value in [
        item.identifier.as_deref(),
        item.md5.as_deref(),
        item.filename.as_deref().map(file_stem),
        Some(file_stem(resolved_filename)),
    ] {
        if let Some(value) = value.filter(|value| !value.is_empty()) {
            if !stems.iter().any(|existing| existing == value) {
                stems.push(value.to_string());
            }
        }
    }

    stems
}

fn file_stem(filename: &str) -> &str {
    filename
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(filename)
}

fn normalize_media_key(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn resolve_media_for_item(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    media_index: &HashMap<String, String>,
    item: &DayOneMedia,
) -> Option<(String, Vec<u8>)> {
    for key in [
        item.identifier.as_deref(),
        item.md5.as_deref(),
        item.filename.as_deref().map(file_stem),
    ] {
        if let Some(key) = key.filter(|key| !key.is_empty()) {
            if let Some(result) = read_media_attachment(archive, media_index, key) {
                return Some(result);
            }
        }
    }
    None
}

/// Map a file extension to a MIME content type.
fn mime_from_extension(ext: &str) -> &'static str {
    match ext.to_lowercase().as_str() {
        "jpeg" | "jpg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "heic" => "image/heic",
        "heif" => "image/heif",
        "tiff" | "tif" => "image/tiff",
        "webp" => "image/webp",
        "mov" => "video/quicktime",
        "mp4" => "video/mp4",
        "m4v" => "video/x-m4v",
        "m4a" => "audio/mp4",
        "mp3" => "audio/mpeg",
        "aac" => "audio/aac",
        "caf" => "audio/x-caf",
        "wav" => "audio/wav",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
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
/// Returns `None` for title when no heading is found or the heading is empty.
fn extract_title_and_body(text: &str) -> (Option<String>, String) {
    let trimmed = text.trim_start();

    if let Some(rest) = trimmed.strip_prefix("# ") {
        if let Some(newline_pos) = rest.find('\n') {
            let title = rest[..newline_pos].trim().to_string();
            let body = rest[newline_pos + 1..].to_string();
            (if title.is_empty() { None } else { Some(title) }, body)
        } else {
            let title = rest.trim().to_string();
            (
                if title.is_empty() { None } else { Some(title) },
                String::new(),
            )
        }
    } else {
        (None, text.to_string())
    }
}

/// Derive a title from the entry date and first non-empty body line.
///
/// Produces titles like "2024-01-15 Went to the store today" (truncated to 60 chars).
fn derive_title(date: Option<chrono::DateTime<chrono::Utc>>, body: &str) -> String {
    let date_part = date
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "Undated".to_string());

    let first_line = body
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim();

    if first_line.is_empty() {
        return date_part;
    }

    // Truncate to ~60 chars on a word boundary.
    let truncated = if first_line.len() > 60 {
        let cut = &first_line[..60];
        // Try to break on a word boundary.
        match cut.rfind(' ') {
            Some(pos) if pos > 30 => format!("{}...", &cut[..pos]),
            _ => format!("{cut}..."),
        }
    } else {
        first_line.to_string()
    };

    format!("{date_part} {truncated}")
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
        assert_eq!(title, Some("My Title".to_string()));
        assert_eq!(body, "Body here.");

        let (title, body) = extract_title_and_body("# Just a title");
        assert_eq!(title, Some("Just a title".to_string()));
        assert_eq!(body, "");

        let (title, body) = extract_title_and_body("No heading here.");
        assert_eq!(title, None);
        assert_eq!(body, "No heading here.");

        let (title, body) = extract_title_and_body("# \nBody after empty heading.");
        assert_eq!(title, None);
        assert_eq!(body, "Body after empty heading.");
    }

    #[test]
    fn test_derive_title() {
        use chrono::TimeZone;
        let dt = chrono::Utc.with_ymd_and_hms(2020, 1, 15, 0, 0, 0).unwrap();
        assert_eq!(
            derive_title(Some(dt), "Went to the store today."),
            "2020-01-15 Went to the store today."
        );
        assert_eq!(derive_title(Some(dt), ""), "2020-01-15");
        assert_eq!(derive_title(None, "Some text"), "Undated Some text");
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

        let result = parse_dayone(json.as_bytes());
        assert!(result.journal_name.is_none());
        assert_eq!(result.entries.len(), 1);

        let entry = result.entries.into_iter().next().unwrap().unwrap();
        assert_eq!(entry.title, "Hello World");
        assert_eq!(entry.body, "This is a test.");
        assert!(entry.date.is_some());
        assert_eq!(
            entry.metadata.get("uuid").unwrap(),
            &YamlValue::String("ABC123".to_string())
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

        let result = parse_dayone(json.as_bytes());
        let entry = result.entries.into_iter().next().unwrap().unwrap();

        assert_eq!(entry.title, "Tagged Entry");
        assert_eq!(
            entry.metadata.get("starred").unwrap(),
            &YamlValue::Bool(true)
        );

        let tags = entry.metadata.get("tags").unwrap();
        if let YamlValue::Sequence(seq) = tags {
            assert_eq!(seq.len(), 2);
        } else {
            panic!("tags should be a sequence");
        }

        assert_eq!(
            entry.metadata.get("location").unwrap(),
            &YamlValue::String("Austin, TX, United States".to_string())
        );
        assert_eq!(
            entry.metadata.get("weather").unwrap(),
            &YamlValue::String("Sunny, 30°C".to_string())
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

        let result = parse_dayone(json.as_bytes());
        let entry = result.entries.into_iter().next().unwrap().unwrap();

        assert_eq!(entry.title, "Hello.");
        assert_eq!(entry.body, "Body with escapes. And !exclamation!");
    }

    #[test]
    fn test_parse_invalid_json() {
        let result = parse_dayone(b"not json");
        assert_eq!(result.entries.len(), 1);
        assert!(result.entries[0].is_err());
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

        let result = parse_dayone(json.as_bytes());
        let entry = result.entries.into_iter().next().unwrap().unwrap();
        assert_eq!(entry.title, "2020-01-01 Just some text without a heading.");
        assert_eq!(entry.body, "Just some text without a heading.");
    }

    #[test]
    fn test_mime_from_extension() {
        assert_eq!(mime_from_extension("jpeg"), "image/jpeg");
        assert_eq!(mime_from_extension("jpg"), "image/jpeg");
        assert_eq!(mime_from_extension("JPG"), "image/jpeg");
        assert_eq!(mime_from_extension("png"), "image/png");
        assert_eq!(mime_from_extension("heic"), "image/heic");
        assert_eq!(mime_from_extension("mov"), "video/quicktime");
        assert_eq!(mime_from_extension("mp4"), "video/mp4");
        assert_eq!(mime_from_extension("m4a"), "audio/mp4");
        assert_eq!(mime_from_extension("pdf"), "application/pdf");
        assert_eq!(mime_from_extension("xyz"), "application/octet-stream");
    }

    #[test]
    fn test_parse_dayone_auto_json() {
        let json = r##"{
            "metadata": { "version": "1.0" },
            "entries": [{
                "text": "# Auto Test\nPlain JSON.",
                "creationDate": "2020-01-01T00:00:00Z"
            }]
        }"##;

        let result = parse_dayone_auto(json.as_bytes());
        assert!(result.journal_name.is_none());
        assert_eq!(result.entries.len(), 1);
        let entry = result.entries.into_iter().next().unwrap().unwrap();
        assert_eq!(entry.title, "Auto Test");
    }

    #[test]
    fn test_parse_dayone_zip_with_media() {
        use std::io::Write;

        let journal_json = r##"{
            "metadata": { "version": "1.0" },
            "entries": [{
                "uuid": "TEST123",
                "text": "# Photo Entry\nA photo.",
                "creationDate": "2020-06-15T12:00:00Z",
                "photos": [{ "identifier": "ABCDEF", "type": "jpeg" }]
            }]
        }"##;

        let mut buf = Vec::new();
        {
            let cursor = Cursor::new(&mut buf);
            let mut zip = zip::ZipWriter::new(cursor);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("My Journal.json", options).unwrap();
            zip.write_all(journal_json.as_bytes()).unwrap();
            zip.start_file("photos/ABCDEF.jpeg", options).unwrap();
            zip.write_all(b"fake-jpeg-data").unwrap();
            zip.finish().unwrap();
        }

        let result = parse_dayone_auto(&buf);
        assert_eq!(result.journal_name.as_deref(), Some("My Journal"));
        assert_eq!(result.entries.len(), 1);
        let entry = result.entries.into_iter().next().unwrap().unwrap();
        assert_eq!(entry.title, "Photo Entry");
        assert_eq!(entry.attachments.len(), 1);
        assert_eq!(entry.attachments[0].filename, "ABCDEF.jpeg");
        assert_eq!(entry.attachments[0].content_type, "image/jpeg");
        assert_eq!(entry.attachments[0].data, b"fake-jpeg-data");
    }

    #[test]
    fn test_parse_dayone_zip_resolves_media_by_md5() {
        use std::io::Write;

        let journal_json = r##"{
            "metadata": { "version": "1.0" },
            "entries": [{
                "uuid": "TEST123",
                "text": "# Photo Entry\n![](dayone-moment://7FB485B610104E7F9A5B785B170035CF)",
                "creationDate": "2020-06-15T12:00:00Z",
                "photos": [{
                    "identifier": "7FB485B610104E7F9A5B785B170035CF",
                    "md5": "1e1768e844003e8585e4472a4c109265",
                    "filename": "IMG_2860.JPG",
                    "type": "jpeg"
                }]
            }]
        }"##;

        let mut buf = Vec::new();
        {
            let cursor = Cursor::new(&mut buf);
            let mut zip = zip::ZipWriter::new(cursor);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("Journal.json", options).unwrap();
            zip.write_all(journal_json.as_bytes()).unwrap();
            zip.start_file("photos/1e1768e844003e8585e4472a4c109265.jpeg", options)
                .unwrap();
            zip.write_all(b"fake-md5-jpeg-data").unwrap();
            zip.finish().unwrap();
        }

        let result = parse_dayone_auto(&buf);
        let entry = result.entries.into_iter().next().unwrap().unwrap();
        assert_eq!(entry.attachments.len(), 1);
        assert_eq!(
            entry.attachments[0].filename,
            "1e1768e844003e8585e4472a4c109265.jpeg"
        );
        assert_eq!(entry.attachments[0].data, b"fake-md5-jpeg-data");
        assert!(
            entry
                .body
                .contains("_attachments/1e1768e844003e8585e4472a4c109265.jpeg")
        );
    }

    #[test]
    fn test_parse_dayone_zip_missing_media() {
        use std::io::Write;

        let journal_json = r##"{
            "metadata": { "version": "1.0" },
            "entries": [{
                "text": "# Missing Photo\nThe photo file is not in the ZIP.",
                "creationDate": "2020-01-01T00:00:00Z",
                "photos": [{ "identifier": "NOTFOUND", "type": "jpeg" }]
            }]
        }"##;

        let mut buf = Vec::new();
        {
            let cursor = Cursor::new(&mut buf);
            let mut zip = zip::ZipWriter::new(cursor);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("Journal.json", options).unwrap();
            zip.write_all(journal_json.as_bytes()).unwrap();
            zip.finish().unwrap();
        }

        let result = parse_dayone_auto(&buf);
        assert_eq!(result.journal_name.as_deref(), Some("Journal"));
        assert_eq!(result.entries.len(), 1);
        let entry = result.entries.into_iter().next().unwrap().unwrap();
        assert_eq!(entry.attachments.len(), 1);
        assert_eq!(entry.attachments[0].filename, "NOTFOUND.jpeg");
        assert_eq!(entry.attachments[0].content_type, "image/jpeg");
        assert!(entry.attachments[0].data.is_empty());
    }

    #[test]
    fn test_parse_dayone_zip_no_json() {
        use std::io::Write;

        let mut buf = Vec::new();
        {
            let cursor = Cursor::new(&mut buf);
            let mut zip = zip::ZipWriter::new(cursor);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("photos/ABC.jpeg", options).unwrap();
            zip.write_all(b"data").unwrap();
            zip.finish().unwrap();
        }

        let result = parse_dayone_auto(&buf);
        assert!(result.journal_name.is_none());
        assert_eq!(result.entries.len(), 1);
        assert!(result.entries[0].is_err());
        assert!(
            result.entries[0]
                .as_ref()
                .unwrap_err()
                .contains("does not contain")
        );
    }

    #[test]
    fn test_dayone_moment_url_replacement() {
        use std::io::Write;

        let journal_json = r##"{
            "metadata": { "version": "1.0" },
            "entries": [{
                "text": "# Test\n![](dayone-moment://ABCDEF)\nSome text.\n![](dayone-moment:/GHIJKL)",
                "creationDate": "2020-01-01T00:00:00Z",
                "photos": [
                    { "identifier": "ABCDEF", "type": "jpeg" },
                    { "identifier": "GHIJKL", "type": "png" }
                ]
            }]
        }"##;

        let mut buf = Vec::new();
        {
            let cursor = Cursor::new(&mut buf);
            let mut zip = zip::ZipWriter::new(cursor);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("Test.json", options).unwrap();
            zip.write_all(journal_json.as_bytes()).unwrap();
            zip.start_file("photos/ABCDEF.jpeg", options).unwrap();
            zip.write_all(b"jpeg-data").unwrap();
            zip.start_file("photos/GHIJKL.png", options).unwrap();
            zip.write_all(b"png-data").unwrap();
            zip.finish().unwrap();
        }

        let result = parse_dayone_auto(&buf);
        let entry = result.entries.into_iter().next().unwrap().unwrap();
        assert!(
            entry.body.contains("_attachments/ABCDEF.jpeg"),
            "body should contain resolved path: {}",
            entry.body
        );
        assert!(
            entry.body.contains("_attachments/GHIJKL.png"),
            "body should contain resolved path: {}",
            entry.body
        );
        assert!(
            !entry.body.contains("dayone-moment"),
            "body should not contain dayone-moment: {}",
            entry.body
        );
    }
}
