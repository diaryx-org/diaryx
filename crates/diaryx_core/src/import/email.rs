//! Email import — parse `.eml` files and `.mbox` archives into [`ImportedEntry`] values.
//!
//! [`parse_eml`] is a pure parser (no I/O). [`parse_mbox`] requires a file
//! path because `mbox-reader` uses memory-mapped I/O internally.

use std::path::Path;

use indexmap::IndexMap;
use mailparse::{MailHeaderMap, ParsedMail, parse_mail};
use serde_yaml::Value;

use super::{ImportedAttachment, ImportedEntry};

/// Parse a single `.eml` file's bytes into an [`ImportedEntry`].
pub fn parse_eml(bytes: &[u8]) -> Result<ImportedEntry, String> {
    let parsed = parse_mail(bytes).map_err(|e| format!("Failed to parse email: {e}"))?;

    let subject = parsed
        .headers
        .get_first_value("Subject")
        .unwrap_or_default();
    let title = if subject.trim().is_empty() {
        "Untitled Email".to_string()
    } else {
        subject
    };

    let date = extract_date(&parsed);

    let mut metadata = IndexMap::new();
    if let Some(from) = parsed.headers.get_first_value("From") {
        metadata.insert("from".to_string(), Value::String(from));
    }
    if let Some(to) = parsed.headers.get_first_value("To") {
        metadata.insert("to".to_string(), Value::String(to));
    }
    if let Some(cc) = parsed.headers.get_first_value("Cc") {
        metadata.insert("cc".to_string(), Value::String(cc));
    }

    let (body_text, attachments) = extract_body_and_attachments(&parsed)?;

    Ok(ImportedEntry {
        title,
        date,
        body: body_text,
        metadata,
        attachments,
    })
}

/// Parse all messages in an `.mbox` file, returning one result per message.
///
/// Requires a file path because `mbox-reader` uses memory-mapped I/O.
pub fn parse_mbox(path: &Path) -> Vec<Result<ImportedEntry, String>> {
    let mbox = match mbox_reader::MboxFile::from_file(path) {
        Ok(mbox) => mbox,
        Err(e) => return vec![Err(format!("Failed to open mbox file: {e}"))],
    };

    mbox.iter()
        .enumerate()
        .filter_map(|(i, entry)| {
            let msg_bytes = entry.message()?;
            Some(parse_eml(msg_bytes).map_err(|e| format!("Message {}: {e}", i + 1)))
        })
        .collect()
}

/// Extract a UTC datetime from the Date header.
fn extract_date(parsed: &ParsedMail) -> Option<chrono::DateTime<chrono::Utc>> {
    let date_str = parsed.headers.get_first_value("Date")?;
    let timestamp = mailparse::dateparse(&date_str).ok()?;
    chrono::DateTime::from_timestamp(timestamp, 0)
}

/// Walk the MIME tree to find the best text body and collect attachments.
fn extract_body_and_attachments(
    parsed: &ParsedMail,
) -> Result<(String, Vec<ImportedAttachment>), String> {
    let mut plain_text: Option<String> = None;
    let mut html_text: Option<String> = None;
    let mut attachments = Vec::new();

    collect_parts(parsed, &mut plain_text, &mut html_text, &mut attachments)?;

    let body = if let Some(text) = plain_text {
        text
    } else if let Some(html) = html_text {
        html_to_markdown(&html)
    } else {
        String::new()
    };

    Ok((body, attachments))
}

/// Recursively walk MIME parts, collecting text bodies and attachment data.
fn collect_parts(
    part: &ParsedMail,
    plain_text: &mut Option<String>,
    html_text: &mut Option<String>,
    attachments: &mut Vec<ImportedAttachment>,
) -> Result<(), String> {
    let content_type = part.ctype.mimetype.to_lowercase();

    if !part.subparts.is_empty() {
        // Multipart container — recurse into children
        for sub in &part.subparts {
            collect_parts(sub, plain_text, html_text, attachments)?;
        }
        return Ok(());
    }

    // Check if this is an attachment (Content-Disposition: attachment, or non-text inline)
    let disposition = part
        .headers
        .get_first_value("Content-Disposition")
        .unwrap_or_default()
        .to_lowercase();
    let is_attachment = disposition.starts_with("attachment");

    if is_attachment
        || (!content_type.starts_with("text/") && !content_type.starts_with("multipart/"))
    {
        // Treat as attachment
        let data = part
            .get_body_raw()
            .map_err(|e| format!("Failed to decode attachment: {e}"))?;
        if !data.is_empty() {
            let filename = extract_attachment_filename(part)
                .unwrap_or_else(|| format!("attachment-{}", attachments.len() + 1));
            attachments.push(ImportedAttachment {
                filename,
                content_type: content_type.clone(),
                data,
            });
        }
        return Ok(());
    }

    // Text part — decode body
    let body = part
        .get_body()
        .map_err(|e| format!("Failed to decode body: {e}"))?;

    if content_type == "text/plain" && plain_text.is_none() {
        *plain_text = Some(body);
    } else if content_type == "text/html" && html_text.is_none() {
        *html_text = Some(body);
    }

    Ok(())
}

/// Try to extract a filename from Content-Disposition or Content-Type parameters.
fn extract_attachment_filename(part: &ParsedMail) -> Option<String> {
    // Try Content-Disposition filename parameter
    if let Some(disp) = part.headers.get_first_value("Content-Disposition") {
        if let Some(name) = extract_param(&disp, "filename") {
            return Some(name);
        }
    }
    // Fall back to Content-Type name parameter
    if let Some(ct) = part.headers.get_first_value("Content-Type") {
        if let Some(name) = extract_param(&ct, "name") {
            return Some(name);
        }
    }
    None
}

/// Extract a named parameter value from a header like `attachment; filename="foo.pdf"`.
fn extract_param(header_value: &str, param_name: &str) -> Option<String> {
    let lower = header_value.to_lowercase();
    let pattern = format!("{}=", param_name);
    let idx = lower.find(&pattern)?;
    let rest = &header_value[idx + pattern.len()..];
    let rest = rest.trim();

    if rest.starts_with('"') {
        // Quoted value
        let end = rest[1..].find('"')?;
        Some(rest[1..1 + end].to_string())
    } else {
        // Unquoted — up to next semicolon or end
        let end = rest.find(';').unwrap_or(rest.len());
        Some(rest[..end].trim().to_string())
    }
}

/// Convert HTML to markdown, falling back to the raw HTML on error.
#[cfg(not(target_arch = "wasm32"))]
fn html_to_markdown(html: &str) -> String {
    html_to_markdown_rs::convert(html, None).unwrap_or_else(|_| html.to_string())
}

#[cfg(target_arch = "wasm32")]
fn html_to_markdown(html: &str) -> String {
    // html-to-markdown-rs is not available on WASM; return raw HTML
    html.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_eml() {
        let eml = b"From: alice@example.com\r\n\
                     To: bob@example.com\r\n\
                     Subject: Hello World\r\n\
                     Date: Mon, 15 Jan 2024 10:30:00 +0000\r\n\
                     Content-Type: text/plain\r\n\
                     \r\n\
                     This is the body.\r\n";

        let entry = parse_eml(eml).unwrap();
        assert_eq!(entry.title, "Hello World");
        assert_eq!(
            entry.metadata.get("from").unwrap(),
            &Value::String("alice@example.com".to_string())
        );
        assert_eq!(
            entry.metadata.get("to").unwrap(),
            &Value::String("bob@example.com".to_string())
        );
        assert!(entry.body.contains("This is the body."));
        assert!(entry.attachments.is_empty());
        assert!(entry.date.is_some());
    }

    #[test]
    fn parse_eml_missing_subject() {
        let eml = b"From: alice@example.com\r\n\
                     Content-Type: text/plain\r\n\
                     \r\n\
                     No subject here.\r\n";

        let entry = parse_eml(eml).unwrap();
        assert_eq!(entry.title, "Untitled Email");
    }

    #[test]
    fn parse_mbox_nonexistent_file() {
        let results = parse_mbox(Path::new("/tmp/nonexistent-diaryx-test.mbox"));
        assert_eq!(results.len(), 1);
        assert!(results[0].is_err());
    }

    #[test]
    fn extract_param_quoted() {
        let val = r#"attachment; filename="report.pdf""#;
        assert_eq!(
            extract_param(val, "filename"),
            Some("report.pdf".to_string())
        );
    }

    #[test]
    fn extract_param_unquoted() {
        let val = "attachment; filename=report.pdf";
        assert_eq!(
            extract_param(val, "filename"),
            Some("report.pdf".to_string())
        );
    }
}
