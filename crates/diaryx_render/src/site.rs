//! Server-side site reconstruction and rendering (ARK Layer 3, Phase 2).
//!
//! Rebuilds [`PublishedPage`]s from stored markdown **sources** and renders the
//! whole site, mirroring the publish plugin's page-derivation rules so the
//! server can render-on-write. The stored sources are already audience-scoped
//! and visibility-filtered (Layer 2), but pre-template — so the per-page
//! pipeline here is: parse → template → preprocess → comrak → transform_links →
//! page assembly. Gated behind the `templating` feature.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use diaryx_core::frontmatter;
use diaryx_core::link_parser;
use diaryx_core::yaml::Value as YamlValue;
use indexmap::IndexMap;

use crate::html::{HtmlRenderer, SiteStyle};
use crate::nav::{build_site_nav_tree, nav_for_page};
use crate::types::{NavLink, PublishedPage};
use crate::{links, markdown, page, template};

/// A stored markdown source to render.
pub struct SourceDoc {
    /// Canonical workspace-relative path including the `.md` extension, e.g.
    /// `"subdir/child.md"`. No leading slash.
    pub path: String,
    /// Raw markdown (frontmatter + visibility-filtered, pre-template body), as
    /// stored by Layer 2.
    pub markdown: String,
    /// Whether this is the workspace root/index page (renders to `index.html`).
    pub is_root: bool,
}

/// A fully rendered page: its output filename, HTML, and ARK blade (if any).
pub struct RenderedPage {
    /// Destination filename, e.g. `"index.html"` or `"subdir/child.html"`.
    pub dest_filename: String,
    /// The complete HTML document.
    pub html: String,
    /// The page's ARK blade (frontmatter `id`), if present.
    pub file_ark: Option<String>,
}

/// Options controlling a site render.
pub struct SiteOptions {
    /// Target audience (used for template `viewer_audience` variables).
    pub audience: Option<String>,
    /// Site title override; defaults to the root page's title.
    pub site_title: Option<String>,
    /// Base URL for sitemap/canonical/feeds; when empty those are skipped.
    pub base_url: Option<String>,
    /// Generate SEO meta + sitemap/robots.
    pub generate_seo: bool,
    /// Generate Atom/RSS feeds + feed `<link>` tags.
    pub generate_feeds: bool,
    /// Caller-supplied appearance (theme/custom CSS/custom favicon).
    pub style: SiteStyle,
}

impl Default for SiteOptions {
    fn default() -> Self {
        Self {
            audience: None,
            site_title: None,
            base_url: None,
            generate_seo: true,
            generate_feeds: true,
            style: SiteStyle::default(),
        }
    }
}

/// The result of rendering a site: the pages plus the static/supplementary
/// assets (`style.css`, favicon, `sitemap.xml`, `robots.txt`, feeds).
pub struct SiteRender {
    /// Rendered pages.
    pub pages: Vec<RenderedPage>,
    /// `(filename, bytes)` assets to write alongside the pages.
    pub assets: Vec<(String, Vec<u8>)>,
}

/// Reconstruct [`PublishedPage`]s from stored sources, fully rendering each
/// page's `rendered_body` (template → preprocess → comrak → link rewrite).
pub fn build_pages(sources: &[SourceDoc], audience: Option<&str>) -> Vec<PublishedPage> {
    // Map sanitized canonical `.md` path → output `.html` filename (root →
    // index.html). Sources are keyed by their workspace-relative path; we
    // sanitize keys so that frontmatter links (which may carry unsanitized
    // characters) resolve against them.
    let mut path_to_filename: HashMap<PathBuf, String> = HashMap::new();
    for s in sources {
        let dest = if s.is_root {
            "index.html".to_string()
        } else {
            output_filename(&s.path)
        };
        path_to_filename.insert(PathBuf::from(sanitize_rel_path(&s.path)), dest);
    }

    // Map sanitized canonical `.md` path → frontmatter title (for contents/
    // parent titles).
    let mut title_map: HashMap<PathBuf, String> = HashMap::new();
    for s in sources {
        if let Ok(parsed) = frontmatter::parse_or_empty(&s.markdown) {
            if let Some(t) = frontmatter::get_string(&parsed.frontmatter, "title") {
                title_map.insert(PathBuf::from(sanitize_rel_path(&s.path)), t.to_string());
            }
        }
    }

    sources
        .iter()
        .map(|s| build_page(s, audience, &path_to_filename, &title_map))
        .collect()
}

/// Reconstruct and render a whole site from stored sources.
pub fn render_site(sources: &[SourceDoc], opts: &SiteOptions) -> SiteRender {
    let pages = build_pages(sources, opts.audience.as_deref());

    let renderer = HtmlRenderer::with_style(opts.style.clone());
    let nav_tree = build_site_nav_tree(&pages);

    let site_title = opts
        .site_title
        .clone()
        .or_else(|| pages.iter().find(|p| p.is_root).map(|p| p.title.clone()))
        .unwrap_or_else(|| "Site".to_string());
    let base_url = opts.base_url.as_deref().unwrap_or("");

    let mut out_pages = Vec::with_capacity(pages.len());
    for p in &pages {
        let nav = nav_for_page(&nav_tree, &p.dest_filename, &pages);
        let seo = if opts.generate_seo {
            page::generate_seo_meta(p, &site_title, base_url)
        } else {
            String::new()
        };
        let feeds = if opts.generate_feeds {
            page::generate_feed_link_tags(&links::root_prefix(&p.dest_filename))
        } else {
            String::new()
        };
        let html = renderer.render_page_with_context(p, &site_title, false, &nav, &seo, &feeds);
        out_pages.push(RenderedPage {
            dest_filename: p.dest_filename.clone(),
            html,
            file_ark: p.file_ark.clone(),
        });
    }

    // Static assets (style.css + favicon) always; supplementary files need a base URL.
    let mut assets = renderer.static_assets();
    if !base_url.is_empty() {
        if opts.generate_seo {
            assets.push((
                "sitemap.xml".to_string(),
                page::generate_sitemap(&pages, base_url).into_bytes(),
            ));
            assets.push((
                "robots.txt".to_string(),
                page::generate_robots_txt(base_url, true).into_bytes(),
            ));
        }
        if opts.generate_feeds {
            let root = pages.iter().find(|p| p.is_root);
            let desc = root.and_then(|r| r.description.as_deref()).unwrap_or("");
            let author = root.and_then(|r| r.author.as_deref()).unwrap_or("");
            assets.push((
                "feed.xml".to_string(),
                page::generate_atom_feed(&pages, &site_title, base_url, desc, author).into_bytes(),
            ));
            assets.push((
                "rss.xml".to_string(),
                page::generate_rss_feed(&pages, &site_title, base_url, desc, author).into_bytes(),
            ));
        }
    }

    SiteRender {
        pages: out_pages,
        assets,
    }
}

// ── Per-page reconstruction ─────────────────────────────────────────────────

fn build_page(
    s: &SourceDoc,
    audience: Option<&str>,
    path_to_filename: &HashMap<PathBuf, String>,
    title_map: &HashMap<PathBuf, String>,
) -> PublishedPage {
    let parsed = frontmatter::parse_or_empty(&s.markdown).unwrap_or(frontmatter::ParsedFile {
        frontmatter: IndexMap::new(),
        body: s.markdown.clone(),
    });
    let fm = &parsed.frontmatter;

    let current_path = PathBuf::from(&s.path);
    let dest_filename = path_to_filename
        .get(&PathBuf::from(sanitize_rel_path(&s.path)))
        .cloned()
        .unwrap_or_else(|| output_filename(&s.path));

    let title = frontmatter::get_string(fm, "title")
        .map(String::from)
        .unwrap_or_else(|| {
            Path::new(&s.path)
                .file_stem()
                .and_then(|x| x.to_str())
                .unwrap_or("Untitled")
                .to_string()
        });

    let contents_links: Vec<NavLink> = frontmatter::get_string_array(fm, "contents")
        .into_iter()
        .map(|child| resolve_link(&child, &current_path, path_to_filename, title_map))
        .collect();

    let parent_link = frontmatter::get_string(fm, "part_of")
        .map(|p| resolve_link(p, &current_path, path_to_filename, title_map));

    // The stored body is already visibility-filtered; template rendering still
    // needs to run (sources are stored pre-template). `template::render*`
    // re-applies visibility (a no-op now) then interpolates handlebars.
    let file_path = Path::new(&s.path);
    let rendered_body = match audience {
        Some(a) => template::render_for_audience(&parsed.body, fm, file_path, None, a),
        None => template::render(&parsed.body, fm, file_path, None),
    }
    .unwrap_or_else(|_| parsed.body.clone());

    // Markdown → HTML, then rewrite internal `.md` links. The empty workspace
    // dir means canonical paths are used directly as `path_to_filename` keys.
    let preprocessed = markdown::preprocess_custom_syntax(&rendered_body);
    let converted = markdown::markdown_to_html(&preprocessed);
    let final_html = links::transform_links(
        &converted,
        file_path,
        path_to_filename,
        Path::new(""),
        &dest_filename,
    );

    let nav_order = fm.get("nav_order").and_then(|v| match v {
        YamlValue::Int(i) => Some(*i as i32),
        YamlValue::Float(f) => Some(*f as i32),
        YamlValue::String(st) => st.parse::<i32>().ok(),
        _ => None,
    });

    PublishedPage {
        source_path: current_path,
        dest_filename,
        title,
        rendered_body: final_html,
        markdown_body: rendered_body,
        contents_links,
        parent_link,
        is_root: s.is_root,
        description: frontmatter::get_string(fm, "description").map(String::from),
        author: frontmatter::get_string(fm, "author").map(String::from),
        created: frontmatter::get_string(fm, "created").map(String::from),
        updated: frontmatter::get_string(fm, "updated").map(String::from),
        attachments: frontmatter::get_string_array(fm, "attachments"),
        nav_title: frontmatter::get_string(fm, "nav_title").map(String::from),
        nav_order,
        hide_from_nav: fm
            .get("hide_from_nav")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        hide_from_feed: fm
            .get("hide_from_feed")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        file_ark: frontmatter::get_string(fm, "id").map(String::from),
        source_markdown: s.markdown.clone(),
    }
}

/// Resolve a `contents`/`part_of` link string to a [`NavLink`] whose href is the
/// target's output `.html` filename and whose title comes from the target's
/// frontmatter, the link text, or a filename-derived fallback.
fn resolve_link(
    link_str: &str,
    current_relative: &Path,
    path_to_filename: &HashMap<PathBuf, String>,
    title_map: &HashMap<PathBuf, String>,
) -> NavLink {
    let parsed = link_parser::parse_link(link_str);
    let canonical = link_parser::to_canonical(&parsed, current_relative);
    // Sanitize so links carrying unsanitized characters resolve against the
    // sanitized source-path keys.
    let key = PathBuf::from(sanitize_rel_path(&canonical));

    let href = path_to_filename
        .get(&key)
        .cloned()
        .unwrap_or_else(|| output_filename(&canonical));

    let title = title_map
        .get(&key)
        .cloned()
        .or_else(|| parsed.title.clone())
        .unwrap_or_else(|| filename_to_title(&canonical));

    NavLink { href, title }
}

// ── Filename helpers (ported from the publish plugin) ────────────────────────

/// Convert a canonical `.md` path to its sanitized `.html` output filename.
fn output_filename(canonical_md: &str) -> String {
    let with_ext = Path::new(canonical_md).with_extension("html");
    let sanitized: PathBuf = with_ext
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

/// Sanitize a single path component for safe use in URLs. Keeps alphanumerics,
/// spaces, dots, hyphens, and underscores; strips URL-unsafe characters.
fn sanitize_path_component(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_' || *c == '.')
        .collect()
}

/// Sanitize each component of a relative path, preserving its extension. Used to
/// normalize both stored source paths and resolved frontmatter links to a common
/// key form (the same sanitization the publish client applies to dest names).
fn sanitize_rel_path(path: &str) -> String {
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

/// Convert a filename to a display title (snake/kebab case → Title Case).
fn filename_to_title(filename: &str) -> String {
    let stem = Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename);

    stem.split(['_', '-'])
        .filter(|s| !s.is_empty())
        .map(|word| {
            let mut chars: Vec<char> = word.chars().collect();
            if let Some(first) = chars.first_mut() {
                *first = first.to_ascii_uppercase();
            }
            chars.into_iter().collect::<String>()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(path: &str, markdown: &str, is_root: bool) -> SourceDoc {
        SourceDoc {
            path: path.to_string(),
            markdown: markdown.to_string(),
            is_root,
        }
    }

    #[test]
    fn output_filename_sanitizes_and_sets_html() {
        assert_eq!(output_filename("notes/My Note!.md"), "notes/My Note.html");
        assert_eq!(output_filename("a/b/c.md"), "a/b/c.html");
    }

    #[test]
    fn filename_to_title_titlecases() {
        assert_eq!(filename_to_title("hello-world.md"), "Hello World");
        assert_eq!(filename_to_title("my_cool_note.md"), "My Cool Note");
    }

    #[test]
    fn build_pages_derives_graph_and_renders() {
        let index = "---\ntitle: Home\ncontents:\n  - \"[Child](/child.md)\"\n---\nWelcome to {{ title }}.\n";
        let child = "---\ntitle: Child Page\npart_of: \"/index.md\"\n---\nSee [home](/index.md) and a ==highlight==.\n";

        let sources = vec![src("index.md", index, true), src("child.md", child, false)];
        let pages = build_pages(&sources, None);

        let home = pages.iter().find(|p| p.is_root).unwrap();
        let kid = pages.iter().find(|p| !p.is_root).unwrap();

        // dest filenames
        assert_eq!(home.dest_filename, "index.html");
        assert_eq!(kid.dest_filename, "child.html");

        // template rendered {{ title }}
        assert!(home.rendered_body.contains("Welcome to Home."));

        // contents_links resolved to child's html + frontmatter title
        assert_eq!(home.contents_links.len(), 1);
        assert_eq!(home.contents_links[0].href, "child.html");
        assert_eq!(home.contents_links[0].title, "Child Page");

        // parent_link resolves back to index
        let parent = kid.parent_link.as_ref().unwrap();
        assert_eq!(parent.href, "index.html");
        assert_eq!(parent.title, "Home");

        // internal .md link rewritten to .html, and custom syntax expanded
        assert!(kid.rendered_body.contains(r#"href="index.html""#));
        assert!(kid.rendered_body.contains("highlight-mark"));
    }

    #[test]
    fn root_by_workspace_name_and_special_chars_resolve() {
        // Option 1: sources keyed by workspace path; root keeps its real name
        // ("Welcome.md"), not "index". Child links reference workspace paths,
        // including a special character that the dest sanitizer strips.
        let root = "---\ntitle: Home\ncontents:\n  - \"/My Note!.md\"\n---\nHi.\n";
        let note = "---\ntitle: My Note\npart_of: \"/Welcome.md\"\n---\nBody.\n";

        let sources = vec![
            src("Welcome.md", root, true),
            src("My Note.md", note, false), // stored under sanitized workspace path
        ];
        let pages = build_pages(&sources, None);

        let home = pages.iter().find(|p| p.is_root).unwrap();
        let note_page = pages.iter().find(|p| !p.is_root).unwrap();

        // Root renders to index.html despite its workspace name.
        assert_eq!(home.dest_filename, "index.html");
        // Child's contents link (with "!") resolves to the sanitized dest + title.
        assert_eq!(home.contents_links.len(), 1);
        assert_eq!(home.contents_links[0].href, "My Note.html");
        assert_eq!(home.contents_links[0].title, "My Note");
        // Child's part_of points at the root by its workspace name → index.html.
        let parent = note_page.parent_link.as_ref().unwrap();
        assert_eq!(parent.href, "index.html");
        assert_eq!(parent.title, "Home");
    }

    #[test]
    fn render_site_produces_pages_nav_and_assets() {
        let index = "---\ntitle: Home\ncontents:\n  - \"[Child](/child.md)\"\n---\nHi.\n";
        let child = "---\ntitle: Child\npart_of: \"/index.md\"\n---\nKid.\n";
        let sources = vec![src("index.md", index, true), src("child.md", child, false)];

        let out = render_site(&sources, &SiteOptions::default());

        assert_eq!(out.pages.len(), 2);
        // index page carries the site nav with a link to the child
        let home = out
            .pages
            .iter()
            .find(|p| p.dest_filename == "index.html")
            .unwrap();
        assert!(home.html.contains("site-nav"));
        assert!(home.html.contains("child.html"));
        assert!(home.html.contains("<!DOCTYPE html>"));

        // assets include the stylesheet
        assert!(out.assets.iter().any(|(n, _)| n == "style.css"));
    }
}
