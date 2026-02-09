//! Publishing functionality for diaryx workspaces
//!
//! Converts workspace markdown files to HTML for sharing.
//!
//! # Async-first Design
//!
//! This module uses `AsyncFileSystem` for all filesystem operations.
//! For synchronous contexts (CLI, tests), wrap a sync filesystem with
//! `SyncToAsyncFs` and use `futures_lite::future::block_on()`.

mod types;

// Re-export types for backwards compatibility
pub use types::{NavLink, PublishOptions, PublishResult, PublishedPage};

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::entry::slugify;
use crate::error::{DiaryxError, Result};
use crate::export::{ExportPlan, Exporter};
use crate::frontmatter;
use crate::fs::AsyncFileSystem;
use crate::link_parser;
use crate::workspace::Workspace;

/// Publisher for converting workspace to HTML (async-first)
pub struct Publisher<FS: AsyncFileSystem> {
    fs: FS,
}

impl<FS: AsyncFileSystem + Clone> Publisher<FS> {
    /// Create a new publisher
    pub fn new(fs: FS) -> Self {
        Self { fs }
    }

    /// Publish a workspace to HTML
    /// Only available on native platforms (not WASM) since it writes to the filesystem
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn publish(
        &self,
        workspace_root: &Path,
        destination: &Path,
        options: &PublishOptions,
    ) -> Result<PublishResult> {
        // Collect files to publish
        let pages = if let Some(ref audience) = options.audience {
            self.collect_with_audience(workspace_root, destination, audience)
                .await?
        } else {
            self.collect_all(workspace_root).await?
        };

        if pages.is_empty() {
            return Ok(PublishResult {
                pages: vec![],
                files_processed: 0,
            });
        }

        let files_processed = pages.len();

        // Generate output
        if options.single_file {
            self.write_single_file(&pages, destination, options).await?;
        } else {
            self.write_multi_file(&pages, destination, options).await?;
        }

        Ok(PublishResult {
            pages,
            files_processed,
        })
    }

    /// Collect all workspace files without audience filtering
    async fn collect_all(&self, workspace_root: &Path) -> Result<Vec<PublishedPage>> {
        let workspace = Workspace::new(self.fs.clone());
        let mut files = workspace.collect_workspace_files(workspace_root).await?;

        // Ensure the workspace root is always first (it becomes index.html)
        // collect_workspace_files sorts alphabetically, so we need to move root to front
        let root_canonical = workspace_root
            .canonicalize()
            .unwrap_or_else(|_| workspace_root.to_path_buf());
        if let Some(pos) = files
            .iter()
            .position(|p| p.canonicalize().unwrap_or_else(|_| p.clone()) == root_canonical)
            && pos != 0
        {
            let root_file = files.remove(pos);
            files.insert(0, root_file);
        }

        let workspace_dir = workspace_root.parent().unwrap_or(workspace_root);
        let mut pages = Vec::new();
        let mut path_to_filename: HashMap<PathBuf, String> = HashMap::new();

        // First pass: assign filenames
        for (idx, file_path) in files.iter().enumerate() {
            let filename = if idx == 0 {
                "index.html".to_string()
            } else {
                self.path_to_html_filename(file_path, workspace_dir)
            };
            path_to_filename.insert(file_path.to_path_buf(), filename);
        }

        // Second pass: process files
        for (idx, file_path) in files.iter().enumerate() {
            if let Some(page) = self
                .process_file(file_path, idx == 0, &path_to_filename, workspace_root)
                .await?
            {
                pages.push(page);
            }
        }

        Ok(pages)
    }

    /// Collect files with audience filtering
    async fn collect_with_audience(
        &self,
        workspace_root: &Path,
        destination: &Path,
        audience: &str,
    ) -> Result<Vec<PublishedPage>> {
        let exporter = Exporter::new(self.fs.clone());
        let plan = exporter
            .plan_export(workspace_root, audience, destination)
            .await?;

        let workspace_dir = workspace_root.parent().unwrap_or(workspace_root);
        let mut pages = Vec::new();
        let mut path_to_filename: HashMap<PathBuf, String> = HashMap::new();

        // First pass: assign filenames
        for (idx, export_file) in plan.included.iter().enumerate() {
            let filename = if idx == 0 {
                "index.html".to_string()
            } else {
                self.path_to_html_filename(&export_file.source_path, workspace_dir)
            };
            path_to_filename.insert(export_file.source_path.clone(), filename);
        }

        // Second pass: process files
        for (idx, export_file) in plan.included.iter().enumerate() {
            if let Some(page) = self
                .process_file(
                    &export_file.source_path,
                    idx == 0,
                    &path_to_filename,
                    workspace_root,
                )
                .await?
            {
                // Filter out excluded children from contents_links
                let filtered_page = self.filter_contents_links(page, &plan, workspace_dir);
                pages.push(filtered_page);
            }
        }

        Ok(pages)
    }

    /// Filter contents links to only include files that are in the export plan
    fn filter_contents_links(
        &self,
        mut page: PublishedPage,
        plan: &ExportPlan,
        workspace_dir: &Path,
    ) -> PublishedPage {
        let included_filenames: std::collections::HashSet<String> = plan
            .included
            .iter()
            .map(|f| self.path_to_html_filename(&f.source_path, workspace_dir))
            .collect();

        // Also include index.html for the root
        let mut allowed = included_filenames;
        allowed.insert("index.html".to_string());

        page.contents_links
            .retain(|link| allowed.contains(&link.href));

        page
    }

    /// Process a single file into a PublishedPage
    async fn process_file(
        &self,
        path: &Path,
        is_root: bool,
        path_to_filename: &HashMap<PathBuf, String>,
        workspace_root: &Path,
    ) -> Result<Option<PublishedPage>> {
        let workspace_dir = workspace_root.parent().unwrap_or(workspace_root);
        let content = match self.fs.read_to_string(path).await {
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
        let title = frontmatter::get_string(&parsed.frontmatter, "title")
            .map(String::from)
            .unwrap_or_else(|| {
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Untitled")
                    .to_string()
            });

        let dest_filename = path_to_filename
            .get(path)
            .cloned()
            .unwrap_or_else(|| self.path_to_html_filename(path, workspace_dir));

        // Build contents links
        let contents_links = self
            .build_contents_links(&parsed.frontmatter, path, path_to_filename, workspace_dir)
            .await;

        // Build parent link
        let parent_link = self
            .build_parent_link(&parsed.frontmatter, path, path_to_filename, workspace_dir)
            .await;

        // Convert markdown to HTML and transform .md links to .html
        let html_body = self.markdown_to_html(&parsed.body);
        let html_body = self.transform_html_links(
            &html_body,
            path,
            path_to_filename,
            workspace_dir,
            &dest_filename,
        );

        Ok(Some(PublishedPage {
            source_path: path.to_path_buf(),
            dest_filename,
            title,
            html_body,
            markdown_body: parsed.body,
            contents_links,
            parent_link,
            is_root,
        }))
    }

    /// Build navigation links from contents property
    async fn build_contents_links(
        &self,
        fm: &indexmap::IndexMap<String, serde_yaml::Value>,
        current_path: &Path,
        path_to_filename: &HashMap<PathBuf, String>,
        workspace_dir: &Path,
    ) -> Vec<NavLink> {
        let contents = frontmatter::get_string_array(fm, "contents");
        // to_canonical expects workspace-relative paths, not absolute
        let current_relative = current_path
            .strip_prefix(workspace_dir)
            .unwrap_or(current_path);

        let mut links = Vec::new();
        for child_ref in contents {
            let parsed = link_parser::parse_link(&child_ref);
            let canonical = link_parser::to_canonical(&parsed, current_relative);
            // Rejoin with workspace_dir to get absolute path for path_to_filename lookup
            let child_path = workspace_dir.join(&canonical);

            let href = path_to_filename
                .get(&child_path)
                .cloned()
                .unwrap_or_else(|| self.path_to_html_filename(&child_path, workspace_dir));

            let title = self
                .get_title_from_file(&child_path)
                .await
                .or_else(|| parsed.title.clone())
                .unwrap_or_else(|| self.filename_to_title(&canonical));

            links.push(NavLink { href, title });
        }
        links
    }

    /// Build parent navigation link from part_of property
    async fn build_parent_link(
        &self,
        fm: &indexmap::IndexMap<String, serde_yaml::Value>,
        current_path: &Path,
        path_to_filename: &HashMap<PathBuf, String>,
        workspace_dir: &Path,
    ) -> Option<NavLink> {
        let part_of = frontmatter::get_string(fm, "part_of")?;
        // to_canonical expects workspace-relative paths, not absolute
        let current_relative = current_path
            .strip_prefix(workspace_dir)
            .unwrap_or(current_path);

        let parsed = link_parser::parse_link(part_of);
        let canonical = link_parser::to_canonical(&parsed, current_relative);
        let parent_path = workspace_dir.join(&canonical);

        let href = path_to_filename
            .get(&parent_path)
            .cloned()
            .unwrap_or_else(|| self.path_to_html_filename(&parent_path, workspace_dir));

        let title = self
            .get_title_from_file(&parent_path)
            .await
            .or_else(|| parsed.title.clone())
            .unwrap_or_else(|| self.filename_to_title(&canonical));

        Some(NavLink { href, title })
    }

    /// Get title from a file's frontmatter
    async fn get_title_from_file(&self, path: &Path) -> Option<String> {
        let content = self.fs.read_to_string(path).await.ok()?;
        let parsed = frontmatter::parse_or_empty(&content).ok()?;
        frontmatter::get_string(&parsed.frontmatter, "title").map(String::from)
    }

    /// Convert a source file path to an HTML output path relative to the destination.
    ///
    /// Preserves the directory structure from the workspace, only changing the extension.
    /// e.g. `workspace/notes/file.md` → `notes/file.html`
    fn path_to_html_filename(&self, path: &Path, workspace_dir: &Path) -> String {
        let relative = path.strip_prefix(workspace_dir).unwrap_or(path);
        relative
            .with_extension("html")
            .to_string_lossy()
            .into_owned()
    }

    /// Convert a filename to a display title
    fn filename_to_title(&self, filename: &str) -> String {
        let stem = Path::new(filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(filename);

        // Convert snake_case or kebab-case to Title Case
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

    /// Convert markdown to HTML using comrak
    #[cfg(feature = "markdown")]
    fn markdown_to_html(&self, markdown: &str) -> String {
        use comrak::{Options, markdown_to_html};

        let mut options = Options::default();
        options.extension.strikethrough = true;
        options.extension.table = true;
        options.extension.autolink = true;
        options.extension.tasklist = true;
        options.render.r#unsafe = true; // Allow raw HTML

        markdown_to_html(markdown, &options)
    }

    #[cfg(not(feature = "markdown"))]
    fn markdown_to_html(&self, markdown: &str) -> String {
        // Basic fallback without comrak
        format!("<pre>{}</pre>", markdown)
    }

    /// Transform links in rendered HTML from `.md` paths to `.html` filenames.
    ///
    /// After comrak converts markdown to HTML, links like `<a href="./sibling.md">`
    /// still point to `.md` files. This rewrites them to point to the corresponding
    /// `.html` output files, matching the Astro site template's remark plugin behavior.
    ///
    /// Uses `link_parser` to resolve paths the same way frontmatter links are resolved:
    /// hrefs are parsed as plain paths and resolved to canonical (workspace-relative)
    /// form, then looked up in the path_to_filename map.
    fn transform_html_links(
        &self,
        html: &str,
        current_path: &Path,
        path_to_filename: &HashMap<PathBuf, String>,
        workspace_dir: &Path,
        dest_filename: &str,
    ) -> String {
        let prefix = Self::root_prefix(dest_filename);
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
                    // Use link_parser to resolve the href to a canonical
                    // (workspace-relative) path, then look up the HTML filename
                    let parsed = link_parser::parse_link(raw_href);
                    let canonical = link_parser::to_canonical(&parsed, current_relative);
                    let target_path = workspace_dir.join(&canonical);

                    let html_path =
                        path_to_filename
                            .get(&target_path)
                            .cloned()
                            .unwrap_or_else(|| {
                                // Fallback: just swap .md → .html on the canonical path
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

    /// Write multiple HTML files
    #[cfg(not(target_arch = "wasm32"))]
    async fn write_multi_file(
        &self,
        pages: &[PublishedPage],
        destination: &Path,
        options: &PublishOptions,
    ) -> Result<()> {
        // Create destination directory
        self.fs.create_dir_all(destination).await?;

        let site_title = options.title.clone().unwrap_or_else(|| {
            pages
                .first()
                .map(|p| p.title.clone())
                .unwrap_or_else(|| "Journal".to_string())
        });

        for page in pages {
            let html = self.render_page(page, &site_title, false);
            let dest_path = destination.join(&page.dest_filename);

            // Create subdirectories as needed (dest_filename may contain paths)
            if let Some(parent) = dest_path.parent() {
                self.fs.create_dir_all(parent).await?;
            }

            self.fs.write_file(&dest_path, &html).await?;

            // Write root page under its original filename too, so both
            // localhost/ and localhost/readme.html (or similar) work
            if page.is_root && page.dest_filename == "index.html" {
                let original_filename = page
                    .source_path
                    .with_extension("html")
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned());
                if let Some(name) = original_filename
                    && name != "index.html"
                {
                    let alias_path = destination.join(&name);
                    self.fs.write_file(&alias_path, &html).await?;
                }
            }
        }

        // Write CSS file
        let css_path = destination.join("style.css");
        self.fs.write_file(&css_path, Self::get_css()).await?;

        Ok(())
    }

    /// Write a single HTML file containing all pages
    #[cfg(not(target_arch = "wasm32"))]
    async fn write_single_file(
        &self,
        pages: &[PublishedPage],
        destination: &Path,
        options: &PublishOptions,
    ) -> Result<()> {
        let site_title = options.title.clone().unwrap_or_else(|| {
            pages
                .first()
                .map(|p| p.title.clone())
                .unwrap_or_else(|| "Journal".to_string())
        });

        let html = self.render_single_file(pages, &site_title);

        // Ensure parent directory exists
        if let Some(parent) = destination.parent() {
            self.fs.create_dir_all(parent).await?;
        }

        self.fs.write_file(destination, &html).await?;

        Ok(())
    }

    /// Compute the relative prefix to get from a page back to the site root.
    /// e.g. `notes/file.html` → `../`, `deeply/nested/page.html` → `../../`, `index.html` → ``
    fn root_prefix(dest_filename: &str) -> String {
        let depth = dest_filename.matches('/').count();
        if depth == 0 {
            String::new()
        } else {
            "../".repeat(depth)
        }
    }

    /// Render a single page to HTML
    fn render_page(&self, page: &PublishedPage, site_title: &str, single_file: bool) -> String {
        let prefix = Self::root_prefix(&page.dest_filename);
        let css_link = if single_file {
            format!("<style>{}</style>", Self::get_css())
        } else {
            format!(r#"<link rel="stylesheet" href="{}style.css">"#, prefix)
        };

        // Split navigation into breadcrumb (above title) and children (after content)
        let breadcrumb_html = self.render_breadcrumb(page, single_file);
        let children_html = self.render_children(page, single_file);

        format!(
            r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{page_title} - {site_title}</title>
    {css_link}
</head>
<body>
    <header>
        <h1 class="site-title"><a href="{root_prefix}index.html">{site_title}</a></h1>
    </header>
    <main>
        <article>
            {breadcrumb}
            <h1 class="page-title">{page_title}</h1>
            <div class="content">
                {content}
            </div>
            {children}
        </article>
    </main>
    <footer>
        <p>Generated by <a href="https://github.com/diaryx-org/diaryx-core">diaryx</a></p>
    </footer>
</body>
</html>"#,
            page_title = html_escape(&page.title),
            site_title = html_escape(site_title),
            root_prefix = prefix,
            css_link = css_link,
            breadcrumb = breadcrumb_html,
            content = page.html_body,
            children = children_html,
        )
    }

    /// Render breadcrumb navigation (parent link above the title)
    fn render_breadcrumb(&self, page: &PublishedPage, single_file: bool) -> String {
        let prefix = Self::root_prefix(&page.dest_filename);
        if let Some(ref parent) = page.parent_link {
            let href = if single_file {
                format!("#{}", self.title_to_anchor(&parent.title))
            } else {
                format!("{}{}", prefix, parent.href)
            };
            format!(
                r#"<nav class="breadcrumb" aria-label="Breadcrumb"><a href="{}">{}</a></nav>"#,
                html_escape(&href),
                html_escape(&parent.title),
            )
        } else {
            String::new()
        }
    }

    /// Render child page links (contents list after the body)
    fn render_children(&self, page: &PublishedPage, single_file: bool) -> String {
        if page.contents_links.is_empty() {
            return String::new();
        }

        let prefix = Self::root_prefix(&page.dest_filename);
        let mut html = String::from(r#"<nav class="page-children" aria-label="Child pages"><ul>"#);
        for link in &page.contents_links {
            let href = if single_file {
                format!("#{}", self.title_to_anchor(&link.title))
            } else {
                format!("{}{}", prefix, link.href)
            };
            html.push_str(&format!(
                r#"<li><a href="{}">{}</a></li>"#,
                html_escape(&href),
                html_escape(&link.title)
            ));
        }
        html.push_str("</ul></nav>");
        html
    }

    /// Render all pages into a single HTML file
    fn render_single_file(&self, pages: &[PublishedPage], site_title: &str) -> String {
        let mut sections = Vec::new();

        for page in pages {
            let anchor = self.title_to_anchor(&page.title);
            let breadcrumb = self.render_breadcrumb(page, true);
            let children = self.render_children(page, true);

            sections.push(format!(
                r#"<section id="{anchor}">
    {breadcrumb}
    <h2 class="page-title">{title}</h2>
    <div class="content">
        {content}
    </div>
    {children}
</section>"#,
                anchor = html_escape(&anchor),
                breadcrumb = breadcrumb,
                title = html_escape(&page.title),
                content = page.html_body,
                children = children,
            ));
        }

        // Build table of contents
        let mut toc = String::from(r#"<nav class="toc"><h2>Table of Contents</h2><ul>"#);
        for page in pages {
            let anchor = self.title_to_anchor(&page.title);
            toc.push_str(&format!(
                r##"<li><a href="#{}">{}</a></li>"##,
                html_escape(&anchor),
                html_escape(&page.title)
            ));
        }
        toc.push_str("</ul></nav>");

        format!(
            r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{site_title}</title>
    <style>{css}</style>
</head>
<body>
    <header>
        <h1 class="site-title">{site_title}</h1>
    </header>
    <main>
        {toc}
        {sections}
    </main>
    <footer>
        <p>Generated by <a href="https://github.com/diaryx-org/diaryx-core">diaryx</a></p>
    </footer>
</body>
</html>"#,
            site_title = html_escape(site_title),
            css = Self::get_css(),
            toc = toc,
            sections = sections.join("\n<hr>\n"),
        )
    }

    /// Convert a title to an anchor ID
    fn title_to_anchor(&self, title: &str) -> String {
        slugify(title)
    }

    /// Get the CSS stylesheet
    fn get_css() -> &'static str {
        r#"
:root {
    --bg: #fafafa;
    --text: #333;
    --text-muted: #666;
    --accent: #2563eb;
    --accent-hover: #1d4ed8;
    --border: #e5e7eb;
    --code-bg: #f3f4f6;
}

@media (prefers-color-scheme: dark) {
    :root {
        --bg: #1a1a1a;
        --text: #e5e5e5;
        --text-muted: #a3a3a3;
        --accent: #60a5fa;
        --accent-hover: #93c5fd;
        --border: #404040;
        --code-bg: #262626;
    }
}

* {
    box-sizing: border-box;
}

html {
    font-size: 16px;
}

body {
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Oxygen, Ubuntu, sans-serif;
    line-height: 1.6;
    color: var(--text);
    background: var(--bg);
    max-width: 48rem;
    margin: 0 auto;
    padding: 2rem 1rem;
}

header {
    margin-bottom: 2rem;
    padding-bottom: 1rem;
    border-bottom: 1px solid var(--border);
}

.site-title {
    font-size: 1.5rem;
    margin: 0;
}

.site-title a {
    color: var(--text);
    text-decoration: none;
}

.site-title a:hover {
    color: var(--accent);
}

.page-title {
    font-size: 2rem;
    margin-top: 0;
    margin-bottom: 1rem;
}

.breadcrumb {
    margin-bottom: 0.25rem;
    font-size: 0.85rem;
    color: var(--text-muted);
}

.breadcrumb a {
    color: var(--text-muted);
    text-decoration: none;
}

.breadcrumb a:hover {
    color: var(--accent);
    text-decoration: underline;
}

.page-children {
    margin-top: 2rem;
    padding-top: 1.5rem;
    border-top: 1px solid var(--border);
}

.page-children ul {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
}

.page-children li {
    margin: 0;
}

.page-children li a {
    display: block;
    padding: 0.4rem 0.75rem;
    border: 1px solid var(--border);
    border-radius: 0.375rem;
    font-size: 0.9rem;
    text-decoration: none;
    color: var(--text);
    transition: border-color 0.15s, color 0.15s;
}

.page-children li a:hover {
    border-color: var(--accent);
    color: var(--accent);
}

nav.toc {
    background: var(--code-bg);
    padding: 1.5rem;
    border-radius: 0.5rem;
    margin-bottom: 2rem;
}

nav.toc h2 {
    margin-top: 0;
}

nav.toc ul {
    margin: 0;
    padding-left: 1.5rem;
}

nav.toc li {
    margin: 0.5rem 0;
}

a {
    color: var(--accent);
    text-decoration: none;
}

a:hover {
    color: var(--accent-hover);
    text-decoration: underline;
}

.content {
    margin-top: 1.5rem;
}

.content h1, .content h2, .content h3, .content h4, .content h5, .content h6 {
    margin-top: 2rem;
    margin-bottom: 0.5rem;
}

.content p {
    margin: 1rem 0;
}

.content ul, .content ol {
    margin: 1rem 0;
    padding-left: 2rem;
}

.content li {
    margin: 0.25rem 0;
}

.content pre {
    background: var(--code-bg);
    padding: 1rem;
    border-radius: 0.5rem;
    overflow-x: auto;
}

.content code {
    background: var(--code-bg);
    padding: 0.2rem 0.4rem;
    border-radius: 0.25rem;
    font-size: 0.9em;
}

.content pre code {
    background: none;
    padding: 0;
}

.content blockquote {
    border-left: 4px solid var(--border);
    margin: 1rem 0;
    padding-left: 1rem;
    color: var(--text-muted);
}

.content table {
    width: 100%;
    border-collapse: collapse;
    margin: 1rem 0;
}

.content th, .content td {
    border: 1px solid var(--border);
    padding: 0.5rem;
    text-align: left;
}

.content th {
    background: var(--code-bg);
}

.content img {
    max-width: 100%;
    height: auto;
}

hr {
    border: none;
    border-top: 1px solid var(--border);
    margin: 3rem 0;
}

section {
    margin-bottom: 2rem;
}

footer {
    margin-top: 3rem;
    padding-top: 1rem;
    border-top: 1px solid var(--border);
    color: var(--text-muted);
    font-size: 0.9rem;
}

footer a {
    color: var(--text-muted);
}

footer a:hover {
    color: var(--accent);
}

@media (max-width: 600px) {
    html {
        font-size: 14px;
    }

    body {
        padding: 1rem;
    }

    .page-title {
        font-size: 1.5rem;
    }
}
"#
    }
}

/// Escape HTML special characters
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("<script>"), "&lt;script&gt;");
        assert_eq!(html_escape("a & b"), "a &amp; b");
        assert_eq!(html_escape(r#"say "hi""#), "say &quot;hi&quot;");
    }
}
