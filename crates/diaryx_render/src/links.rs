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

    while let Some(href_start) = remaining.find("href=\"") {
        result.push_str(&remaining[..href_start + 6]);
        remaining = &remaining[href_start + 6..];

        if let Some(href_end) = remaining.find('"') {
            let rest = &remaining[href_end..];
            let raw_href = &remaining[..href_end];

            if raw_href.ends_with(".md")
                && !raw_href.starts_with("http://")
                && !raw_href.starts_with("https://")
                && !raw_href.starts_with('#')
            {
                let decoded_href = percent_decode(raw_href);
                let parsed = link_parser::parse_link(&decoded_href);
                let canonical = link_parser::to_canonical(&parsed, current_relative);
                let target_path = workspace_dir.join(&canonical);

                let html_path = path_to_filename
                    .get(&target_path)
                    .cloned()
                    .unwrap_or_else(|| {
                        Path::new(&canonical)
                            .with_extension("html")
                            .to_string_lossy()
                            .into_owned()
                    });

                result.push_str(&format!("{}{}", prefix, html_path));
            } else {
                result.push_str(raw_href);
            }

            remaining = rest;
        }
    }
    result.push_str(remaining);

    result
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
    fn transform_links_unknown_md_falls_back_to_html_extension() {
        let workspace = Path::new("/ws");
        let map = HashMap::new();
        let html = r#"<a href="missing.md">x</a>"#;
        let current = Path::new("/ws/source.md");
        let out = transform_links(html, current, &map, workspace, "source.html");
        assert_eq!(out, r#"<a href="missing.html">x</a>"#);
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
