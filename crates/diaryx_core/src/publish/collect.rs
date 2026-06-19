//! Workspace source collection for server-side rendering.
//!
//! Walks a workspace for a given audience (via [`Exporter`]) and produces the
//! audience-scoped markdown [`SourceFile`]s — frontmatter parsed, body
//! visibility-filtered, sensitive keys stripped — **without** rendering any
//! HTML. The server reconstructs nav/links/HTML from these sources, so the
//! client path stays light (no comrak/handlebars).

use std::path::{Path, PathBuf};

use crate::error::{DiaryxError, Result};
use crate::export::Exporter;
use crate::fs::AsyncFileSystem;
use crate::publish::source::SourceFile;
use crate::{frontmatter, visibility};

/// Top-level frontmatter keys stripped from the uploaded source — it is served
/// publicly via ARK resolution, so internal publishing config must not leak.
/// Author-facing metadata (title, description, dates, `id`, …) is preserved.
const SOURCE_DENYLIST: &[&str] = &["plugins", "audiences", "audiences_migrated"];

/// Collect a single audience's markdown sources from the workspace.
///
/// `default_audience` is the tag assigned to entries with no explicit/inherited
/// audience (`None` ⇒ such entries are private). The workspace root index is
/// emitted first and flagged `is_index` (its dest is `index.html`).
pub async fn collect_audience_sources<FS>(
    fs: FS,
    workspace_root: &Path,
    audience: &str,
    default_audience: Option<&str>,
) -> Result<Vec<SourceFile>>
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
    for (idx, ef) in included.iter().enumerate() {
        if let Some(sf) =
            build_source_file(&fs, &ef.source_path, workspace_dir, audience, idx == 0).await?
        {
            sources.push(sf);
        }
    }
    Ok(sources)
}

async fn build_source_file<FS>(
    fs: &FS,
    path: &Path,
    workspace_dir: &Path,
    audience: &str,
    is_root: bool,
) -> Result<Option<SourceFile>>
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

    Ok(Some(SourceFile {
        source_markdown,
        source_rel_path,
        dest_path,
        file_ark,
        is_index: is_root,
    }))
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
}
