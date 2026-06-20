//! Site layout helpers: root-relative prefixes, percent decoding, and
//! rewriting internal `.md` links to their published `.html` targets.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use diaryx_core::link_parser;

/// Compute the relative prefix to get from a page back to the site root.
///
/// `index.html` → `""`, `a/b.html` → `"../"`, `a/b/c.html` → `"../../"`.
pub fn root_prefix(dest_filename: &str) -> String {
    let depth = dest_filename.matches('/').count();
    if depth == 0 {
        String::new()
    } else {
        "../".repeat(depth)
    }
}

/// Rewrite internal `.md` hyperlinks in rendered HTML to their published
/// `.html` destinations, resolving relative/workspace-root paths via the
/// `path_to_filename` map. External links, anchors, and non-`.md` hrefs are
/// left untouched.
///
/// The lookup key is sanitized the same way `path_to_filename`'s keys are (see
/// [`sanitize_rel_path`]), so a link like `First post!.md` resolves to the
/// stored `First post.html` instead of a fabricated `First post!.html`.
///
/// A link whose target is **not** in this render set (excluded by audience
/// visibility, or simply missing) is stripped: the `<a>` becomes a
/// `<span class="unpublished-link">` that keeps the link text but isn't
/// clickable, so the page never points at something that 404s.
pub fn transform_links(
    html: &str,
    current_path: &Path,
    path_to_filename: &HashMap<PathBuf, String>,
    workspace_dir: &Path,
    dest_filename: &str,
) -> String {
    let prefix = root_prefix(dest_filename);
    // to_canonical expects workspace-relative paths
    let current_relative = current_path
        .strip_prefix(workspace_dir)
        .unwrap_or(current_path);

    let mut result = String::with_capacity(html.len());
    let mut remaining = html;

    while let Some(tag_start) = remaining.find("<a ") {
        // Emit everything before the anchor verbatim.
        result.push_str(&remaining[..tag_start]);
        let after = &remaining[tag_start..];

        // Find the end of the opening tag. comrak escapes `>` inside attribute
        // values, so the first `>` reliably closes the tag.
        let Some(gt) = after.find('>') else {
            result.push_str(after);
            remaining = "";
            break;
        };
        let open_tag = &after[..=gt];
        let tail = &after[gt + 1..];

        // Only internal `.md` links are candidates for rewrite/strip.
        let canonical =
            extract_href(open_tag).and_then(|href| md_link_canonical(href, current_relative));

        match canonical {
            None => {
                // External link, anchor, or non-`.md` target — leave untouched.
                result.push_str(open_tag);
                remaining = tail;
            }
            Some(canonical) => {
                // Anchors can't nest, so the next `</a>` closes this one.
                let Some(close) = tail.find("</a>") else {
                    result.push_str(open_tag);
                    remaining = tail;
                    continue;
                };
                let inner = &tail[..close];
                let after_close = &tail[close + "</a>".len()..];

                let key = workspace_dir.join(sanitize_rel_path(&canonical));
                match path_to_filename.get(&key) {
                    Some(html_path) => {
                        // Published target — rewrite the href, keep the anchor.
                        result
                            .push_str(&replace_href(open_tag, &format!("{}{}", prefix, html_path)));
                        result.push_str(inner);
                        result.push_str("</a>");
                    }
                    None => {
                        // Not in this render set — strip to a marked span.
                        result.push_str(
                            r#"<span class="unpublished-link" title="This page isn’t published">"#,
                        );
                        result.push_str(inner);
                        result.push_str("</span>");
                    }
                }
                remaining = after_close;
            }
        }
    }
    result.push_str(remaining);

    result
}

/// Extract the raw (still percent-encoded) `href="…"` value from an opening tag.
fn extract_href(open_tag: &str) -> Option<&str> {
    let start = open_tag.find("href=\"")? + 6;
    let rest = &open_tag[start..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

/// If `raw_href` is an internal `.md` link, return its workspace-relative
/// canonical path; otherwise `None` (external/anchor/non-`.md` links skipped).
fn md_link_canonical(raw_href: &str, current_relative: &Path) -> Option<String> {
    if !raw_href.ends_with(".md")
        || raw_href.starts_with("http://")
        || raw_href.starts_with("https://")
        || raw_href.starts_with('#')
    {
        return None;
    }
    let decoded = percent_decode(raw_href);
    let parsed = link_parser::parse_link(&decoded);
    Some(link_parser::to_canonical(&parsed, current_relative))
}

/// Replace the `href="…"` value in an opening tag, preserving other attributes.
fn replace_href(open_tag: &str, new_value: &str) -> String {
    let Some(start) = open_tag.find("href=\"") else {
        return open_tag.to_string();
    };
    let value_start = start + 6;
    let rest = &open_tag[value_start..];
    let Some(end) = rest.find('"') else {
        return open_tag.to_string();
    };
    format!("{}{}{}", &open_tag[..value_start], new_value, &rest[end..])
}

/// Sanitize a single path component for safe use in URLs. Keeps alphanumerics,
/// spaces, dots, hyphens, and underscores; strips URL-unsafe characters. Mirrors
/// the publish client's dest-name sanitization so links resolve to the stored
/// filenames.
pub fn sanitize_path_component(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_' || *c == '.')
        .collect()
}

/// Sanitize each component of a relative path, preserving its extension. Used to
/// normalize both stored source paths and resolved frontmatter/body links to a
/// common key form.
pub fn sanitize_rel_path(path: &str) -> String {
    let sanitized: PathBuf = Path::new(path)
        .components()
        .map(|c| match c {
            std::path::Component::Normal(s) => {
                std::ffi::OsString::from(sanitize_path_component(&s.to_string_lossy()))
            }
            other => other.as_os_str().to_owned(),
        })
        .collect();
    sanitized.to_string_lossy().into_owned()
}

/// Decode percent-encoded characters in a URL string (e.g. `%20` → ` `).
pub fn percent_decode(input: &str) -> String {
    let mut result = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2]))
        {
            result.push(hi << 4 | lo);
            i += 3;
            continue;
        }
        result.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(result).unwrap_or_else(|_| input.to_string())
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_prefix_depth() {
        assert_eq!(root_prefix("index.html"), "");
        assert_eq!(root_prefix("a/b.html"), "../");
        assert_eq!(root_prefix("a/b/c.html"), "../../");
    }

    #[test]
    fn percent_decode_cases() {
        assert_eq!(percent_decode("hello"), "hello");
        assert_eq!(percent_decode("hello%20world"), "hello world");
        assert_eq!(
            percent_decode("Message%20for%20my%20family.md"),
            "Message for my family.md"
        );
        assert_eq!(percent_decode("%2Fpath%2Fto%2Ffile"), "/path/to/file");
        // Incomplete sequences are left as-is
        assert_eq!(percent_decode("hello%2"), "hello%2");
        assert_eq!(percent_decode("hello%"), "hello%");
        // Invalid hex chars left as-is
        assert_eq!(percent_decode("hello%ZZ"), "hello%ZZ");
    }

    #[test]
    fn transform_links_rewrites_known_md_target() {
        let workspace = Path::new("/ws");
        let mut map = HashMap::new();
        map.insert(
            PathBuf::from("/ws/notes/target.md"),
            "notes/target.html".to_string(),
        );

        let html = r#"<a href="target.md">x</a>"#;
        let current = Path::new("/ws/notes/source.md");
        let out = transform_links(html, current, &map, workspace, "notes/source.html");
        // depth 1 → prefix "../"
        assert_eq!(out, r#"<a href="../notes/target.html">x</a>"#);
    }

    #[test]
    fn transform_links_unknown_md_is_stripped_and_marked() {
        // A link to a page that isn't in the render set (excluded/missing) must
        // not become a dead .html link — it's stripped to a marked span that
        // keeps the text but isn't clickable.
        let workspace = Path::new("/ws");
        let map = HashMap::new();
        let html = r#"<a href="missing.md">link text</a>"#;
        let current = Path::new("/ws/source.md");
        let out = transform_links(html, current, &map, workspace, "source.html");
        assert_eq!(
            out,
            r#"<span class="unpublished-link" title="This page isn’t published">link text</span>"#
        );
    }

    #[test]
    fn transform_links_resolves_sanitized_target() {
        // The link text references "First post!.md" but the stored/published key
        // is the sanitized "First post.md" → "First post.html". The '!' must not
        // leak into the href (regression for the sanitization-mismatch bug).
        let workspace = Path::new("");
        let mut map = HashMap::new();
        map.insert(
            PathBuf::from("First post.md"),
            "First post.html".to_string(),
        );
        let html = r#"<a href="First%20post!.md">x</a>"#;
        let current = Path::new("source.md");
        let out = transform_links(html, current, &map, workspace, "source.html");
        assert_eq!(out, r#"<a href="First post.html">x</a>"#);
    }

    #[test]
    fn transform_links_preserves_inner_markup_when_stripping() {
        let workspace = Path::new("");
        let map = HashMap::new();
        let html = r#"<a href="gone.md">see <em>this</em></a>"#;
        let current = Path::new("source.md");
        let out = transform_links(html, current, &map, workspace, "source.html");
        assert!(out.contains(r#"<span class="unpublished-link""#));
        assert!(out.contains("see <em>this</em></span>"));
        assert!(!out.contains("<a "));
    }

    #[test]
    fn transform_links_leaves_external_and_anchors() {
        let workspace = Path::new("/ws");
        let map = HashMap::new();
        let current = Path::new("/ws/source.md");
        let html =
            r##"<a href="https://x.com/a.md">e</a><a href="#frag">f</a><a href="img.png">g</a>"##;
        let out = transform_links(html, current, &map, workspace, "source.html");
        assert_eq!(out, html);
    }
}
