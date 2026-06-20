//! Workspace source collection for server-side rendering.
//!
//! Walks a workspace for a given audience (via [`Exporter`]) and produces the
//! audience-scoped markdown [`SourceFile`]s — frontmatter parsed, body
//! visibility-filtered, sensitive keys stripped — plus referenced
//! [`Attachment`]s, **without** rendering any HTML. The server reconstructs
//! nav/links/HTML from these sources, so the client path stays light (no
//! comrak/handlebars).
//!
//! Note: HTML "island" attachments are uploaded as-is here; the resize-bridge
//! injection the plugin applied is a published-output concern left to a later
//! (server-side) step.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::error::{DiaryxError, Result};
use crate::export::Exporter;
use crate::fs::AsyncFileSystem;
use crate::publish::source::{Attachment, SourceFile};
use crate::{frontmatter, link_parser, visibility};

/// Top-level frontmatter keys stripped from the uploaded source — it is served
/// publicly via ARK resolution, so internal publishing config must not leak.
/// Author-facing metadata (title, description, dates, `id`, …) is preserved.
const SOURCE_DENYLIST: &[&str] = &["plugins", "audiences", "audiences_migrated"];

/// A collected audience: its markdown sources and referenced attachments.
#[derive(Debug, Clone, Default)]
pub struct CollectedAudience {
    pub sources: Vec<SourceFile>,
    pub attachments: Vec<Attachment>,
}

/// Collect a single audience's markdown sources + attachments from the workspace.
///
/// `default_audience` is the tag assigned to entries with no explicit/inherited
/// audience (`None` ⇒ such entries are private). The workspace root index is
/// emitted first and flagged `is_index` (its dest is `index.html`).
pub async fn collect_audience<FS>(
    fs: FS,
    workspace_root: &Path,
    audience: &str,
    default_audience: Option<&str>,
) -> Result<CollectedAudience>
where
    FS: AsyncFileSystem + Clone,
{
    let exporter = Exporter::new(fs.clone());
    let plan = exporter
        .plan_export(
            workspace_root,
            audience,
            // Destination is unused for source collection (we never write files).
            Path::new("/diaryx-publish-sources"),
            default_audience,
        )
        .await?;

    let workspace_dir = workspace_root.parent().unwrap_or(workspace_root);

    // plan_export is depth-first post-order (children before parents); move the
    // workspace root to the front so it becomes the index page.
    let mut included = plan.included.clone();
    let root_canonical = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    if let Some(pos) = included.iter().position(|f| {
        f.source_path
            .canonicalize()
            .unwrap_or_else(|_| f.source_path.clone())
            == root_canonical
    }) && pos != 0
    {
        let root_file = included.remove(pos);
        included.insert(0, root_file);
    }

    let mut sources = Vec::with_capacity(included.len());
    let mut attachments = Vec::new();
    let mut seen_attachments: HashSet<String> = HashSet::new();

    for (idx, ef) in included.iter().enumerate() {
        let Some((source, att_refs)) =
            build_source_file(&fs, &ef.source_path, workspace_dir, audience, idx == 0).await?
        else {
            continue;
        };
        sources.push(source);

        // Read referenced attachments once (deduped across pages).
        for canonical in att_refs {
            if !seen_attachments.insert(canonical.clone()) {
                continue;
            }
            let abs = workspace_dir.join(&canonical);
            match fs.read(&abs).await {
                Ok(bytes) => attachments.push(Attachment {
                    mime_type: mime_type_from_ext(Path::new(&canonical)),
                    dest_rel: canonical,
                    bytes,
                }),
                Err(_) => {
                    // Missing/unreadable attachment — skip (matches prior best-effort).
                }
            }
        }
    }

    Ok(CollectedAudience {
        sources,
        attachments,
    })
}

/// Build one source file and resolve its (non-`.md`) attachment references.
async fn build_source_file<FS>(
    fs: &FS,
    path: &Path,
    workspace_dir: &Path,
    audience: &str,
    is_root: bool,
) -> Result<Option<(SourceFile, Vec<String>)>>
where
    FS: AsyncFileSystem,
{
    let content = match fs.read_to_string(path).await {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(DiaryxError::FileRead {
                path: path.to_path_buf(),
                source: e,
            });
        }
    };

    let parsed = frontmatter::parse_or_empty(&content)?;

    // Audience visibility filtering; the server stores this (pre-template) body.
    let filtered_body = visibility::filter_body_for_audience(&parsed.body, audience);

    // Strip sensitive keys before the source is uploaded (served publicly).
    let mut source_fm = parsed.frontmatter.clone();
    for key in SOURCE_DENYLIST {
        source_fm.shift_remove(*key);
    }
    let source_markdown = frontmatter::serialize(&source_fm, &filtered_body)
        .unwrap_or_else(|_| filtered_body.clone());

    let file_ark = frontmatter::get_string(&parsed.frontmatter, "id").map(String::from);

    let rel = path.strip_prefix(workspace_dir).unwrap_or(path);
    // Sources key by their (sanitized) workspace-relative path; only the dest
    // special-cases the root to index.html.
    let source_rel_path = sanitize_rel_path(rel, "md");
    let dest_path = if is_root {
        "index.html".to_string()
    } else {
        sanitize_rel_path(rel, "html")
    };

    // Resolve attachment references: local file refs in the body + the
    // frontmatter `attachments` list, canonicalized against this file, non-`.md`.
    let mut att_refs = Vec::new();
    for raw in extract_local_file_refs(&filtered_body) {
        let link = link_parser::parse_link(&raw);
        let canonical = link_parser::to_canonical(&link, rel);
        if !canonical.ends_with(".md") {
            att_refs.push(canonical);
        }
    }
    for s in frontmatter::get_string_array(&parsed.frontmatter, "attachments") {
        let link = link_parser::parse_link(&s);
        let canonical = link_parser::to_canonical(&link, rel);
        if !canonical.ends_with(".md") {
            att_refs.push(canonical);
        }
    }

    Ok(Some((
        SourceFile {
            source_markdown,
            source_rel_path,
            dest_path,
            file_ark,
            is_index: is_root,
        },
        att_refs,
    )))
}

/// Sanitize each component of a relative path and set its extension. Mirrors the
/// publish dest-filename sanitization (keep alphanumerics, spaces, `-`, `_`, `.`).
fn sanitize_rel_path(rel: &Path, ext: &str) -> String {
    let with_ext = rel.with_extension(ext);
    let sanitized: PathBuf = with_ext
        .components()
        .map(|c| match c {
            std::path::Component::Normal(s) => {
                std::ffi::OsString::from(sanitize_component(&s.to_string_lossy()))
            }
            other => other.as_os_str().to_owned(),
        })
        .collect();
    sanitized.to_string_lossy().into_owned()
}

fn sanitize_component(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_' || *c == '.')
        .collect()
}

/// Whether a reference points at a local file (not external URL/anchor/scheme)
/// and has a file extension.
fn is_local_file_ref(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    if path.starts_with("http://")
        || path.starts_with("https://")
        || path.starts_with('#')
        || path.starts_with("mailto:")
        || path.starts_with("data:")
        || path.starts_with("javascript:")
    {
        return false;
    }
    let filename = path.rsplit('/').next().unwrap_or(path);
    filename.contains('.')
}

/// Extract local file reference paths from markdown: `(path)` link targets and
/// HTML `src`/`href`/`srcset` attributes.
fn extract_local_file_refs(markdown: &str) -> Vec<String> {
    let mut paths = Vec::new();

    let mut remaining = markdown;
    while let Some(paren_pos) = remaining.find('(') {
        remaining = &remaining[paren_pos + 1..];
        if let Some(close) = remaining.find(')') {
            let path = remaining[..close].trim();
            if is_local_file_ref(path) {
                paths.push(path.to_string());
            }
            remaining = &remaining[close + 1..];
        } else {
            break;
        }
    }

    for marker in &["src=\"", "href=\""] {
        let mut remaining = markdown;
        while let Some(pos) = remaining.find(marker) {
            remaining = &remaining[pos + marker.len()..];
            if let Some(end) = remaining.find('"') {
                let path = remaining[..end].trim();
                if is_local_file_ref(path) {
                    paths.push(path.to_string());
                }
                remaining = &remaining[end + 1..];
            } else {
                break;
            }
        }
    }

    let mut remaining = markdown;
    while let Some(pos) = remaining.find("srcset=\"") {
        remaining = &remaining[pos + "srcset=\"".len()..];
        if let Some(end) = remaining.find('"') {
            let srcset = remaining[..end].trim();
            for candidate in srcset.split(',') {
                let candidate = candidate.trim();
                let path = candidate.split_whitespace().next().unwrap_or("").trim();
                if is_local_file_ref(path) {
                    paths.push(path.to_string());
                }
            }
            remaining = &remaining[end + 1..];
        } else {
            break;
        }
    }

    paths
}

/// Guess a content type from a file extension.
fn mime_type_from_ext(path: &Path) -> String {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("html" | "htm") => "text/html",
        Some("svg") => "image/svg+xml",
        Some("webp") => "image/webp",
        Some("pdf") => "application/pdf",
        Some("ico") => "image/x-icon",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("mp3") => "audio/mpeg",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        _ => "application/octet-stream",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn sanitize_rel_path_swaps_ext_and_strips_unsafe() {
        assert_eq!(
            sanitize_rel_path(Path::new("notes/My Note!.md"), "html"),
            "notes/My Note.html"
        );
        assert_eq!(
            sanitize_rel_path(Path::new("notes/My Note!.md"), "md"),
            "notes/My Note.md"
        );
        assert_eq!(
            sanitize_rel_path(Path::new("Welcome.md"), "html"),
            "Welcome.html"
        );
    }

    #[test]
    fn local_file_refs_filter_external_and_extensionless() {
        assert!(is_local_file_ref("img/a.png"));
        assert!(!is_local_file_ref("https://x.com/a.png"));
        assert!(!is_local_file_ref("#anchor"));
        assert!(!is_local_file_ref("just-text"));
    }

    #[test]
    fn extract_refs_from_markdown_and_html() {
        let md = "![a](img/a.png) and <img src=\"b.jpg\"> and [doc](notes/x.md)";
        let refs = extract_local_file_refs(md);
        assert!(refs.contains(&"img/a.png".to_string()));
        assert!(refs.contains(&"b.jpg".to_string()));
        assert!(refs.contains(&"notes/x.md".to_string())); // .md filtered later in caller
    }

    #[test]
    fn mime_types() {
        assert_eq!(mime_type_from_ext(Path::new("a.png")), "image/png");
        assert_eq!(mime_type_from_ext(Path::new("a.html")), "text/html");
        assert_eq!(
            mime_type_from_ext(Path::new("a.bin")),
            "application/octet-stream"
        );
    }
}
