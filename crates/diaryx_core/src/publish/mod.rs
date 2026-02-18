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

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::entry::slugify;
use crate::error::{DiaryxError, Result};
use crate::export::{ExportPlan, Exporter};
use crate::frontmatter;
use crate::fs::AsyncFileSystem;
use crate::link_parser;
use crate::workspace::Workspace;

/// Publisher for converting workspace to HTML (async-first)
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub struct Publisher<FS: AsyncFileSystem> {
    fs: FS,
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
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
                attachments_copied: 0,
            });
        }

        let files_processed = pages.len();
        let workspace_dir = workspace_root.parent().unwrap_or(workspace_root);

        // Generate output
        if options.single_file {
            self.write_single_file(&pages, destination, options).await?;
        } else {
            self.write_multi_file(&pages, destination, options).await?;
        }

        // Copy attachments to output directory
        let mut attachments_copied = 0;
        if options.copy_attachments && !options.single_file {
            let attachments = Self::collect_attachment_paths(&pages, workspace_dir);
            for (src, dest_rel) in &attachments {
                let dest = destination.join(dest_rel);
                if let Some(parent) = dest.parent() {
                    self.fs.create_dir_all(parent).await?;
                }
                match self.fs.read_binary(src).await {
                    Ok(bytes) => {
                        self.fs.write_binary(&dest, &bytes).await?;
                        attachments_copied += 1;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        // Attachment file doesn't exist on disk — skip silently
                    }
                    Err(e) => {
                        return Err(DiaryxError::FileRead {
                            path: src.clone(),
                            source: e,
                        });
                    }
                }
            }
        }

        Ok(PublishResult {
            pages,
            files_processed,
            attachments_copied,
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

        // Ensure the workspace root is first (it becomes index.html).
        // plan_export uses depth-first post-order, so children appear before
        // their parent. We need to move the root to position 0.
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

        // First pass: assign filenames
        for (idx, export_file) in included.iter().enumerate() {
            let filename = if idx == 0 {
                "index.html".to_string()
            } else {
                self.path_to_html_filename(&export_file.source_path, workspace_dir)
            };
            path_to_filename.insert(export_file.source_path.clone(), filename);
        }

        // Second pass: process files
        for (idx, export_file) in included.iter().enumerate() {
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
            frontmatter: parsed.frontmatter.clone(),
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

        let preprocessed = preprocess_custom_syntax(markdown);

        let mut options = Options::default();
        options.extension.strikethrough = true;
        options.extension.table = true;
        options.extension.autolink = true;
        options.extension.tasklist = true;
        options.extension.footnotes = true;
        options.render.r#unsafe = true; // Allow raw HTML

        markdown_to_html(&preprocessed, &options)
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
                    // URL-decode the href first, since comrak percent-encodes
                    // spaces and special characters (e.g. %20 for spaces)
                    let decoded_href = percent_decode(raw_href);
                    // Use link_parser to resolve the href to a canonical
                    // (workspace-relative) path, then look up the HTML filename
                    let parsed = link_parser::parse_link(&decoded_href);
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

    /// Collect non-markdown file paths referenced by published pages.
    ///
    /// Scans each page's markdown body for local file references (images,
    /// PDFs, etc.) and the frontmatter `attachments` list. Returns
    /// deduplicated pairs of `(source_absolute_path, dest_relative_path)`.
    /// Markdown files are excluded since they become HTML pages.
    #[cfg(not(target_arch = "wasm32"))]
    fn collect_attachment_paths(
        pages: &[PublishedPage],
        workspace_dir: &Path,
    ) -> Vec<(PathBuf, PathBuf)> {
        let mut seen = HashSet::new();
        let mut results = Vec::new();

        for page in pages {
            let current_rel = page
                .source_path
                .strip_prefix(workspace_dir)
                .unwrap_or(&page.source_path);

            // Scan markdown body for local file references
            for raw_path in extract_local_file_refs(&page.markdown_body) {
                let parsed = link_parser::parse_link(&raw_path);
                let canonical = link_parser::to_canonical(&parsed, current_rel);
                if !canonical.ends_with(".md") {
                    let src = workspace_dir.join(&canonical);
                    let dest_rel = PathBuf::from(&canonical);
                    if seen.insert(canonical) {
                        results.push((src, dest_rel));
                    }
                }
            }

            // Check frontmatter attachments list
            if let Some(serde_yaml::Value::Sequence(seq)) = page.frontmatter.get("attachments") {
                for item in seq {
                    if let Some(s) = item.as_str() {
                        let parsed = link_parser::parse_link(s);
                        let canonical = link_parser::to_canonical(&parsed, current_rel);
                        if !canonical.ends_with(".md") {
                            let src = workspace_dir.join(&canonical);
                            let dest_rel = PathBuf::from(&canonical);
                            if seen.insert(canonical) {
                                results.push((src, dest_rel));
                            }
                        }
                    }
                }
            }
        }

        results
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

        let breadcrumb_html = self.render_breadcrumb(page, single_file);
        let pill_html = self.render_metadata_pill(page, single_file);

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
            <div class="content">
                {content}
            </div>
        </article>
    </main>
    {pill}
    <footer>
        <p>Generated by <a href="https://github.com/diaryx-org/diaryx-core">diaryx</a></p>
    </footer>
    <script>
    (function() {{
        if ('ontouchstart' in window || navigator.maxTouchPoints > 0) {{
            var pill = document.querySelector('.metadata-pill');
            if (pill) {{
                pill.addEventListener('click', function(e) {{
                    e.stopPropagation();
                    pill.classList.toggle('is-active');
                }});
                document.addEventListener('click', function() {{
                    pill.classList.remove('is-active');
                }});
            }}
        }}
        document.querySelectorAll('.spoiler-mark').forEach(function(el) {{
            el.addEventListener('click', function() {{
                el.classList.toggle('spoiler-hidden');
                el.classList.toggle('spoiler-revealed');
            }});
        }});
    }})();
    </script>
</body>
</html>"#,
            page_title = html_escape(&page.title),
            site_title = html_escape(site_title),
            root_prefix = prefix,
            css_link = css_link,
            breadcrumb = breadcrumb_html,
            content = page.html_body,
            pill = pill_html,
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

    /// Render a serde_yaml::Value as HTML for the metadata pill.
    fn render_frontmatter_value(value: &serde_yaml::Value) -> String {
        match value {
            serde_yaml::Value::String(s) => html_escape(s),
            serde_yaml::Value::Number(n) => html_escape(&n.to_string()),
            serde_yaml::Value::Bool(b) => html_escape(&b.to_string()),
            serde_yaml::Value::Null => "\u{2014}".to_string(), // em-dash
            serde_yaml::Value::Sequence(seq) => seq
                .iter()
                .map(Self::render_frontmatter_value)
                .collect::<Vec<_>>()
                .join("<br>"),
            serde_yaml::Value::Mapping(_) => {
                let yaml = serde_yaml::to_string(value).unwrap_or_default();
                format!("<pre>{}</pre>", html_escape(yaml.trim()))
            }
            serde_yaml::Value::Tagged(t) => Self::render_frontmatter_value(&t.value),
        }
    }

    /// Render the floating metadata pill for a page.
    fn render_metadata_pill(&self, page: &PublishedPage, single_file: bool) -> String {
        if page.frontmatter.is_empty() {
            return String::new();
        }

        let prefix = Self::root_prefix(&page.dest_filename);

        // Build collapsed pill summary: title · author · audience
        let title = frontmatter::get_string(&page.frontmatter, "title");
        let author = frontmatter::get_string(&page.frontmatter, "author");
        let audience_val = page.frontmatter.get("audience");
        let audience_str = audience_val.and_then(|v| match v {
            serde_yaml::Value::String(s) => Some(s.clone()),
            serde_yaml::Value::Sequence(seq) => {
                let parts: Vec<String> = seq
                    .iter()
                    .filter_map(|item| item.as_str().map(String::from))
                    .collect();
                if parts.is_empty() {
                    None
                } else {
                    Some(parts.join(", "))
                }
            }
            _ => None,
        });

        let summary_parts: Vec<&str> = [title, author, audience_str.as_deref()]
            .into_iter()
            .flatten()
            .collect();
        let pill_summary = if summary_parts.is_empty() {
            "Document Info".to_string()
        } else {
            summary_parts.join(" \u{00b7} ") // middle dot
        };

        // Build expanded panel rows
        let mut rows = String::new();
        for (key, value) in &page.frontmatter {
            let rendered_value = if key == "contents" {
                // Render contents links as clickable <a> tags
                if page.contents_links.is_empty() {
                    Self::render_frontmatter_value(value)
                } else {
                    page.contents_links
                        .iter()
                        .map(|link| {
                            let href = if single_file {
                                format!("#{}", self.title_to_anchor(&link.title))
                            } else {
                                format!("{}{}", prefix, link.href)
                            };
                            format!(
                                r#"<a href="{}">{}</a>"#,
                                html_escape(&href),
                                html_escape(&link.title)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("<br>")
                }
            } else if key == "part_of" {
                // Render part_of as clickable <a> tag
                if let Some(ref parent) = page.parent_link {
                    let href = if single_file {
                        format!("#{}", self.title_to_anchor(&parent.title))
                    } else {
                        format!("{}{}", prefix, parent.href)
                    };
                    format!(
                        r#"<a href="{}">{}</a>"#,
                        html_escape(&href),
                        html_escape(&parent.title)
                    )
                } else {
                    Self::render_frontmatter_value(value)
                }
            } else {
                Self::render_frontmatter_value(value)
            };

            rows.push_str(&format!(
                r#"<div class="pill-row"><dt>{}</dt><dd>{}</dd></div>"#,
                html_escape(key),
                rendered_value
            ));
        }

        format!(
            r#"<div class="metadata-pill" role="complementary" aria-label="Document metadata">
    <div class="pill-collapsed"><span class="pill-text">{summary}</span></div>
    <div class="pill-expanded">
        <div class="pill-header"><span class="pill-header-label">Document Info</span></div>
        <div class="pill-content"><dl>{rows}</dl></div>
    </div>
</div>"#,
            summary = html_escape(&pill_summary),
            rows = rows,
        )
    }

    /// Render frontmatter as a collapsible `<details>` block for single-file mode.
    fn render_metadata_details(&self, page: &PublishedPage) -> String {
        if page.frontmatter.is_empty() {
            return String::new();
        }

        let mut rows = String::new();
        for (key, value) in &page.frontmatter {
            let rendered_value = if key == "contents" && !page.contents_links.is_empty() {
                page.contents_links
                    .iter()
                    .map(|link| {
                        let href = format!("#{}", self.title_to_anchor(&link.title));
                        format!(
                            r#"<a href="{}">{}</a>"#,
                            html_escape(&href),
                            html_escape(&link.title)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("<br>")
            } else if key == "part_of" {
                if let Some(ref parent) = page.parent_link {
                    let href = format!("#{}", self.title_to_anchor(&parent.title));
                    format!(
                        r#"<a href="{}">{}</a>"#,
                        html_escape(&href),
                        html_escape(&parent.title)
                    )
                } else {
                    Self::render_frontmatter_value(value)
                }
            } else {
                Self::render_frontmatter_value(value)
            };

            rows.push_str(&format!(
                r#"<div class="pill-row"><dt>{}</dt><dd>{}</dd></div>"#,
                html_escape(key),
                rendered_value
            ));
        }

        format!(
            r#"<details class="metadata-details"><summary>Document Info</summary><dl>{}</dl></details>"#,
            rows
        )
    }

    /// Render all pages into a single HTML file
    fn render_single_file(&self, pages: &[PublishedPage], site_title: &str) -> String {
        let mut sections = Vec::new();

        for page in pages {
            let anchor = self.title_to_anchor(&page.title);
            let breadcrumb = self.render_breadcrumb(page, true);
            let metadata = self.render_metadata_details(page);

            sections.push(format!(
                r#"<section id="{anchor}">
    {breadcrumb}
    {metadata}
    <div class="content">
        {content}
    </div>
</section>"#,
                anchor = html_escape(&anchor),
                breadcrumb = breadcrumb,
                metadata = metadata,
                content = page.html_body,
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
    <script>
    (function() {{
        document.querySelectorAll('.spoiler-mark').forEach(function(el) {{
            el.addEventListener('click', function() {{
                el.classList.toggle('spoiler-hidden');
                el.classList.toggle('spoiler-revealed');
            }});
        }});
    }})();
    </script>
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
    --bg: #fafaf9;
    --text: #0f172a;
    --text-muted: #64748b;
    --accent: #3b82f6;
    --accent-hover: #1d4ed8;
    --border: #e5e7eb;
    --code-bg: #f3f4f6;
    --surface-bg: rgba(255, 255, 255, 0.95);
    --surface-border: rgba(15, 23, 42, 0.08);
    --surface-shadow: 0 1px 3px rgba(15, 23, 42, 0.08), 0 8px 24px rgba(15, 23, 42, 0.06);
    --divider-color: rgba(15, 23, 42, 0.08);
}

@media (prefers-color-scheme: dark) {
    :root {
        --bg: #0a0a0f;
        --text: #f1f5f9;
        --text-muted: #94a3b8;
        --accent: #60a5fa;
        --accent-hover: #93c5fd;
        --border: #334155;
        --code-bg: #1e293b;
        --surface-bg: rgba(17, 24, 39, 0.95);
        --surface-border: rgba(255, 255, 255, 0.1);
        --surface-shadow: 0 1px 3px rgba(0, 0, 0, 0.3), 0 12px 32px rgba(0, 0, 0, 0.4);
        --divider-color: rgba(255, 255, 255, 0.08);
    }
}

* { box-sizing: border-box; }

body {
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
    line-height: 1.7;
    color: var(--text);
    background: var(--bg);
    max-width: 48rem;
    margin: 0 auto;
    padding: 2rem 1rem 4rem;
    font-size: 16px;
    -webkit-font-smoothing: antialiased;
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

/* ── Content typography ── */

a {
    color: var(--accent);
    text-decoration: none;
}

a:hover {
    color: var(--accent-hover);
    text-decoration: underline;
}

.content h1, .content h2, .content h3, .content h4, .content h5, .content h6 {
    margin-top: 2rem;
    margin-bottom: 0.5rem;
    line-height: 1.25;
    letter-spacing: -0.01em;
}

.content h1 { font-size: 2rem; margin-top: 0; }
.content h2 { font-size: 1.5rem; }
.content h3 { font-size: 1.25rem; }

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
    line-height: 1.5;
}

.content code {
    background: var(--code-bg);
    padding: 0.15em 0.4em;
    border-radius: 0.25rem;
    font-size: 0.9em;
    font-family: "SF Mono", "JetBrains Mono", Consolas, monospace;
}

.content pre code {
    background: none;
    padding: 0;
}

.content blockquote {
    border-left: 3px solid var(--accent);
    margin: 1.5rem 0;
    padding-left: 1.25rem;
    color: var(--text-muted);
    font-style: italic;
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
    border-radius: 0.5rem;
}

.content hr {
    border: none;
    border-top: 1px solid var(--border);
    margin: 2.5rem 0;
}

/* ── Floating metadata pill ── */

.metadata-pill {
    position: fixed;
    bottom: 2rem;
    right: 2rem;
    z-index: 1000;
    max-width: 420px;
}

.pill-collapsed {
    display: flex;
    align-items: center;
    padding: 0.75rem 1.125rem;
    background: var(--surface-bg);
    border: 1px solid var(--surface-border);
    border-radius: 999px;
    box-shadow: var(--surface-shadow);
    backdrop-filter: blur(16px);
    -webkit-backdrop-filter: blur(16px);
    cursor: pointer;
    transition: all 0.25s cubic-bezier(0.4, 0, 0.2, 1);
    user-select: none;
}

.metadata-pill:hover .pill-collapsed {
    box-shadow: 0 1px 3px rgba(15, 23, 42, 0.1), 0 12px 32px rgba(15, 23, 42, 0.12);
    transform: translateY(-2px);
}

.pill-text {
    font-size: 0.875rem;
    font-weight: 500;
    line-height: 1.4;
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 320px;
}

.pill-expanded {
    position: absolute;
    bottom: 100%;
    right: 0;
    margin-bottom: 0.25rem;
    width: 380px;
    max-width: calc(100vw - 5rem);
    background: var(--surface-bg);
    border: 1px solid var(--surface-border);
    border-radius: 1rem;
    box-shadow: var(--surface-shadow);
    backdrop-filter: blur(16px);
    -webkit-backdrop-filter: blur(16px);
    opacity: 0;
    visibility: hidden;
    transform: translateY(8px);
    transition: all 0.25s cubic-bezier(0.4, 0, 0.2, 1);
    pointer-events: none;
}

.pill-expanded::after {
    content: "";
    position: absolute;
    top: 100%;
    left: 0;
    right: 0;
    height: 0.5rem;
    background: transparent;
}

.metadata-pill:hover .pill-expanded {
    opacity: 1;
    visibility: visible;
    transform: translateY(0);
    pointer-events: auto;
}

.pill-header {
    padding: 1rem 1.25rem 0.75rem;
    border-bottom: 1px solid var(--divider-color);
}

.pill-header-label {
    font-size: 0.6875rem;
    font-weight: 700;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--text-muted);
}

.pill-content {
    padding: 0.875rem 1.25rem 1.125rem;
    max-height: 60vh;
    overflow-y: auto;
}

.pill-content dl {
    margin: 0;
}

.pill-row {
    margin: 0 0 0.75rem 0;
}

.pill-row:last-child {
    margin-bottom: 0;
}

.pill-content dt {
    margin: 0 0 0.25rem 0;
    font-weight: 600;
    font-size: 0.8125rem;
    text-transform: capitalize;
    color: var(--text-muted);
    letter-spacing: 0.02em;
}

.pill-content dd {
    margin: 0;
    white-space: pre-wrap;
    word-break: break-word;
    font-family: "SF Mono", "JetBrains Mono", Consolas, monospace;
    font-size: 0.8125rem;
    line-height: 1.5;
    color: var(--text);
    padding: 0.4rem 0.65rem;
    background: rgba(0, 0, 0, 0.02);
    border-radius: 0.375rem;
    border: 1px solid var(--divider-color);
}

.pill-content dd a {
    color: var(--accent);
    text-decoration: none;
    font-weight: 500;
}

.pill-content dd a:hover {
    text-decoration: underline;
}

@media (prefers-color-scheme: dark) {
    .metadata-pill:hover .pill-collapsed {
        box-shadow: 0 1px 3px rgba(0, 0, 0, 0.4), 0 16px 40px rgba(0, 0, 0, 0.5);
    }
    .pill-content dd {
        background: rgba(255, 255, 255, 0.03);
    }
}

/* Touch devices: tap to toggle */
@media (hover: none) and (pointer: coarse) {
    .pill-expanded {
        display: none;
    }
    .metadata-pill.is-active .pill-expanded {
        display: block;
        opacity: 1;
        visibility: visible;
        transform: translateY(0);
        pointer-events: auto;
    }
}

/* Mobile: full-width pill */
@media (max-width: 48rem) {
    .metadata-pill {
        bottom: 1rem;
        right: 1rem;
        left: 1rem;
        max-width: none;
    }
    .pill-collapsed {
        padding: 0.625rem 1rem;
    }
    .pill-text {
        font-size: 0.8125rem;
    }
    .pill-expanded {
        position: fixed;
        bottom: 4.5rem;
        left: 1rem;
        right: 1rem;
        margin-bottom: 0;
        width: auto;
    }
    .pill-content {
        max-height: 50vh;
    }
}

@media print {
    .metadata-pill { display: none; }
}

/* ── Single-file details fallback ── */

.metadata-details {
    margin-bottom: 1.5rem;
    border: 1px solid var(--border);
    border-radius: 0.5rem;
    font-size: 0.875rem;
}

.metadata-details summary {
    padding: 0.625rem 1rem;
    cursor: pointer;
    font-weight: 600;
    color: var(--text-muted);
    font-size: 0.8125rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    user-select: none;
}

.metadata-details summary:hover {
    color: var(--accent);
}

.metadata-details dl {
    margin: 0;
    padding: 0.5rem 1rem 1rem;
    border-top: 1px solid var(--divider-color);
}

.metadata-details .pill-row {
    margin: 0 0 0.5rem;
}

.metadata-details dt {
    font-weight: 600;
    font-size: 0.75rem;
    text-transform: capitalize;
    color: var(--text-muted);
    margin-bottom: 0.125rem;
}

.metadata-details dd {
    margin: 0;
    font-size: 0.8125rem;
    color: var(--text);
}

.metadata-details dd a {
    color: var(--accent);
}

/* ── Layout misc ── */

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
    body {
        padding: 1rem 1rem 4rem;
        font-size: 15px;
    }
}

/* ── Colored highlights ── */

.highlight-mark {
    background: none;
    padding: 0.1em 0.2em;
    border-radius: 0.2em;
}

.highlight-red { background: oklch(0.92 0.12 25); }
.highlight-orange { background: oklch(0.93 0.1 60); }
.highlight-yellow { background: oklch(0.95 0.12 95); }
.highlight-green { background: oklch(0.92 0.08 145); }
.highlight-cyan { background: oklch(0.92 0.08 195); }
.highlight-blue { background: oklch(0.88 0.1 250); }
.highlight-violet { background: oklch(0.9 0.1 300); }
.highlight-pink { background: oklch(0.93 0.1 350); }
.highlight-brown { background: oklch(0.88 0.06 60); }
.highlight-grey { background: oklch(0.9 0 0); }

@media (prefers-color-scheme: dark) {
    .highlight-red { background: oklch(0.35 0.12 25); }
    .highlight-orange { background: oklch(0.38 0.1 60); }
    .highlight-yellow { background: oklch(0.42 0.12 95); }
    .highlight-green { background: oklch(0.38 0.08 145); }
    .highlight-cyan { background: oklch(0.38 0.08 195); }
    .highlight-blue { background: oklch(0.35 0.1 250); }
    .highlight-violet { background: oklch(0.38 0.1 300); }
    .highlight-pink { background: oklch(0.4 0.1 350); }
    .highlight-brown { background: oklch(0.38 0.06 60); }
    .highlight-grey { background: oklch(0.4 0 0); }
}

/* ── Spoilers ── */

.spoiler-mark {
    cursor: pointer;
    border-radius: 0.2em;
    padding: 0.1em 0.3em;
    transition: background 0.2s, color 0.2s;
}

.spoiler-hidden {
    background: var(--text);
    color: transparent;
    user-select: none;
}

.spoiler-revealed {
    background: var(--code-bg);
    color: var(--text);
}

/* ── Footnotes ── */

.footnotes {
    margin-top: 3rem;
    padding-top: 1.5rem;
    border-top: 1px solid var(--border);
    font-size: 0.875rem;
    color: var(--text-muted);
}

.footnotes ol {
    padding-left: 1.5rem;
}

.footnotes li {
    margin: 0.5rem 0;
}

.footnote-ref a, .footnotes a {
    color: var(--accent);
    text-decoration: none;
    font-weight: 500;
}

.footnote-ref a:hover, .footnotes a:hover {
    text-decoration: underline;
}

sup.footnote-ref {
    font-size: 0.75em;
    line-height: 0;
    vertical-align: super;
}
"#
    }
}

/// Extract local file reference paths from markdown text.
///
/// Finds references inside markdown link/image syntax `[...](...)`
/// and HTML attributes `src="..."` / `href="..."`. Excludes external
/// URLs, anchors, and data/javascript URIs.
#[cfg(not(target_arch = "wasm32"))]
fn extract_local_file_refs(markdown: &str) -> Vec<String> {
    let mut paths = Vec::new();

    // Find paths in markdown links: [text](path) and ![alt](path)
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

    // Find paths in HTML attributes: src="path" and href="path"
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

    paths
}

/// Returns true if a path looks like a local file reference (not an external
/// URL, anchor, or special URI scheme).
#[cfg(not(target_arch = "wasm32"))]
fn is_local_file_ref(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    // Exclude external URLs, anchors, and special schemes
    if path.starts_with("http://")
        || path.starts_with("https://")
        || path.starts_with('#')
        || path.starts_with("mailto:")
        || path.starts_with("data:")
        || path.starts_with("javascript:")
    {
        return false;
    }
    // Must have a file extension (to avoid matching plain text in parens)
    let filename = path.rsplit('/').next().unwrap_or(path);
    filename.contains('.')
}

/// Escape HTML special characters
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Decode percent-encoded characters in a URL string (e.g. `%20` → ` `).
fn percent_decode(input: &str) -> String {
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

/// Pre-process custom markdown syntax (highlights, spoilers) into raw HTML
/// before passing to comrak. Skips fenced code blocks and inline code.
#[cfg(feature = "markdown")]
fn preprocess_custom_syntax(markdown: &str) -> String {
    let bytes = markdown.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(len);
    let mut i = 0;

    while i < len {
        // Skip fenced code blocks (``` ... ```)
        if i + 2 < len && bytes[i] == b'`' && bytes[i + 1] == b'`' && bytes[i + 2] == b'`' {
            // Copy the opening fence and everything until the closing fence
            let fence_start = i;
            i += 3;
            // Skip to end of opening fence line
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            // Find closing fence
            loop {
                if i >= len {
                    out.push_str(&markdown[fence_start..]);
                    return out;
                }
                if bytes[i] == b'\n'
                    && i + 3 < len
                    && bytes[i + 1] == b'`'
                    && bytes[i + 2] == b'`'
                    && bytes[i + 3] == b'`'
                {
                    // Include the newline + closing fence + rest of line
                    i += 4;
                    while i < len && bytes[i] != b'\n' {
                        i += 1;
                    }
                    break;
                }
                i += 1;
            }
            out.push_str(&markdown[fence_start..i]);
            continue;
        }

        // Skip inline code (` ... `)
        if bytes[i] == b'`' {
            let start = i;
            i += 1;
            while i < len && bytes[i] != b'`' {
                i += 1;
            }
            if i < len {
                i += 1; // consume closing backtick
            }
            out.push_str(&markdown[start..i]);
            continue;
        }

        // Try highlight: ==text== or =={color}text==
        if i + 1 < len && bytes[i] == b'=' && bytes[i + 1] == b'=' {
            if let Some((html, consumed)) = try_parse_highlight(&markdown[i..]) {
                out.push_str(&html);
                i += consumed;
                continue;
            }
        }

        // Try spoiler: ||text||
        if i + 1 < len && bytes[i] == b'|' && bytes[i + 1] == b'|' {
            if let Some((html, consumed)) = try_parse_spoiler(&markdown[i..]) {
                out.push_str(&html);
                i += consumed;
                continue;
            }
        }

        out.push(markdown[i..].chars().next().unwrap());
        i += markdown[i..].chars().next().unwrap().len_utf8();
    }

    out
}

/// Try to parse a highlight starting at `==`. Returns `(html, bytes_consumed)`.
///
/// Matches `==text==` (default yellow) or `=={color}text==`.
/// Valid colors: red, orange, yellow, green, cyan, blue, violet, pink, brown, grey.
#[cfg(feature = "markdown")]
fn try_parse_highlight(s: &str) -> Option<(String, usize)> {
    const VALID_COLORS: &[&str] = &[
        "red", "orange", "yellow", "green", "cyan", "blue", "violet", "pink", "brown", "grey",
    ];

    if !s.starts_with("==") {
        return None;
    }

    let after_open = &s[2..];
    if after_open.is_empty() || after_open.starts_with("==") {
        // Empty highlight `====` — leave unchanged
        return None;
    }

    // Check for optional color: =={color}
    let (color, content_start) = if after_open.starts_with('{') {
        let close_brace = after_open.find('}')?;
        let color_name = &after_open[1..close_brace];
        if !VALID_COLORS.contains(&color_name) {
            return None;
        }
        (color_name, close_brace + 1)
    } else {
        ("yellow", 0)
    };

    let content_region = &after_open[content_start..];
    let close_pos = content_region.find("==")?;
    if close_pos == 0 {
        // No content between opening and closing ==
        return None;
    }

    let content = &content_region[..close_pos];
    // Don't allow newlines in highlights
    if content.contains('\n') {
        return None;
    }

    let total_consumed = 2 + content_start + close_pos + 2;
    let html = format!(
        r#"<mark data-highlight-color="{color}" class="highlight-mark highlight-{color}">{content}</mark>"#,
        color = color,
        content = html_escape(content),
    );

    Some((html, total_consumed))
}

/// Try to parse a spoiler starting at `||`. Returns `(html, bytes_consumed)`.
///
/// Matches `||text||` where content cannot contain `|` or newlines.
#[cfg(feature = "markdown")]
fn try_parse_spoiler(s: &str) -> Option<(String, usize)> {
    if !s.starts_with("||") {
        return None;
    }

    let after_open = &s[2..];
    if after_open.is_empty() || after_open.starts_with("||") {
        // Empty spoiler `||||` — leave unchanged
        return None;
    }

    let close_pos = after_open.find("||")?;
    if close_pos == 0 {
        return None;
    }

    let content = &after_open[..close_pos];
    // Don't allow | or newlines inside spoiler content
    if content.contains('|') || content.contains('\n') {
        return None;
    }

    let total_consumed = 2 + close_pos + 2;
    let html = format!(
        r#"<span data-spoiler="" class="spoiler-mark spoiler-hidden">{content}</span>"#,
        content = html_escape(content),
    );

    Some((html, total_consumed))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "markdown")]
    use crate::fs::FileSystem;

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("<script>"), "&lt;script&gt;");
        assert_eq!(html_escape("a & b"), "a &amp; b");
        assert_eq!(html_escape(r#"say "hi""#), "say &quot;hi&quot;");
    }

    #[test]
    fn test_percent_decode() {
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

    #[cfg(feature = "markdown")]
    #[test]
    fn test_transform_links_no_corruption() {
        let fs = crate::fs::SyncToAsyncFs::new(crate::fs::RealFileSystem);
        let publisher = Publisher::new(fs);

        // Simulate comrak output for: [Click me!](/family.md)
        let html1 = r#"<p><a href="/family.md">Click me!</a></p>"#;
        // Simulate comrak output for: [← Go back](</Message for my family.md>)
        let html2 = r#"<h1>Hooray, you made it!</h1>
<p>That's all folks!</p>
<p><a href="/Message%20for%20my%20family.md">← Go back</a></p>"#;

        let workspace_dir = Path::new("/tmp/workspace");
        let current_path = workspace_dir.join("family.md");
        let mut path_to_filename = HashMap::new();
        path_to_filename.insert(
            workspace_dir.join("Message for my family.md"),
            "index.html".to_string(),
        );
        path_to_filename.insert(workspace_dir.join("family.md"), "family.html".to_string());

        let result1 = publisher.transform_html_links(
            html1,
            &workspace_dir.join("Message for my family.md"),
            &path_to_filename,
            workspace_dir,
            "index.html",
        );
        eprintln!("Result 1: {}", result1);
        assert!(
            result1.contains(">Click me!</a></p>"),
            "Link text corrupted: {}",
            result1
        );
        assert!(
            !result1.contains(" me!</p>") || result1.contains("Click me!</a></p>"),
            "Duplicate text after link: {}",
            result1
        );

        let result2 = publisher.transform_html_links(
            html2,
            &current_path,
            &path_to_filename,
            workspace_dir,
            "family.html",
        );
        eprintln!("Result 2: {}", result2);
        assert!(
            result2.contains("all folks!"),
            "Body text corrupted: {}",
            result2
        );
        assert!(
            result2.contains(">← Go back</a></p>"),
            "Link text corrupted: {}",
            result2
        );
        assert!(
            !result2.contains("stp;"),
            "Spurious text after link: {}",
            result2
        );
    }

    #[test]
    fn test_extract_local_file_refs_markdown() {
        let md = "Some text\n![image](_attachments/photo.png)\n[pdf](./_attachments/doc.pdf)\nno match here";
        let refs = extract_local_file_refs(md);
        assert!(refs.contains(&"_attachments/photo.png".to_string()));
        assert!(refs.contains(&"./_attachments/doc.pdf".to_string()));
    }

    #[test]
    fn test_extract_local_file_refs_non_attachments_folder() {
        let md = "![icon](/public/icon.svg)\n[doc](assets/readme.pdf)";
        let refs = extract_local_file_refs(md);
        assert!(refs.contains(&"/public/icon.svg".to_string()));
        assert!(refs.contains(&"assets/readme.pdf".to_string()));
    }

    #[test]
    fn test_extract_local_file_refs_html_src() {
        let md = r#"<img src="/public/diaryx-icon.svg" alt="icon" style="width: 6rem;">"#;
        let refs = extract_local_file_refs(md);
        assert_eq!(refs.len(), 1);
        assert!(refs.contains(&"/public/diaryx-icon.svg".to_string()));
    }

    #[test]
    fn test_extract_local_file_refs_skips_external_and_anchors() {
        let md = "[link](https://example.com)\n[anchor](#heading)\n[mail](mailto:a@b.com)\nplain text (no file ref)";
        let refs = extract_local_file_refs(md);
        assert!(refs.is_empty());
    }

    #[test]
    fn test_extract_local_file_refs_skips_md_links() {
        // .md links are handled by the HTML link transformer, not attachment copying
        let md = "[sibling](./other.md)";
        let refs = extract_local_file_refs(md);
        // .md files DO get extracted here, but collect_attachment_paths filters them out
        assert!(refs.contains(&"./other.md".to_string()));
    }

    #[test]
    fn test_collect_attachment_paths_deduplicates() {
        let workspace_dir = Path::new("/workspace");
        let pages = vec![PublishedPage {
            source_path: PathBuf::from("/workspace/README.md"),
            dest_filename: "index.html".to_string(),
            title: "Root".to_string(),
            html_body: String::new(),
            markdown_body: "![img](_attachments/a.png)\n![img2](_attachments/a.png)".to_string(),
            contents_links: vec![],
            parent_link: None,
            is_root: true,
            frontmatter: indexmap::IndexMap::new(),
        }];
        let paths = Publisher::<crate::fs::SyncToAsyncFs<crate::fs::InMemoryFileSystem>>::collect_attachment_paths(&pages, workspace_dir);
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].0, PathBuf::from("/workspace/_attachments/a.png"));
        assert_eq!(paths[0].1, PathBuf::from("_attachments/a.png"));
    }

    #[test]
    fn test_collect_attachment_paths_from_frontmatter() {
        let workspace_dir = Path::new("/workspace");
        let mut fm = indexmap::IndexMap::new();
        fm.insert(
            "attachments".to_string(),
            serde_yaml::Value::Sequence(vec![
                serde_yaml::Value::String("_attachments/doc.pdf".to_string()),
                serde_yaml::Value::String("[Icon](/public/icon.svg)".to_string()),
            ]),
        );
        let pages = vec![PublishedPage {
            source_path: PathBuf::from("/workspace/notes/entry.md"),
            dest_filename: "notes/entry.html".to_string(),
            title: "Entry".to_string(),
            html_body: String::new(),
            markdown_body: String::new(),
            contents_links: vec![],
            parent_link: None,
            is_root: false,
            frontmatter: fm,
        }];
        let paths = Publisher::<crate::fs::SyncToAsyncFs<crate::fs::InMemoryFileSystem>>::collect_attachment_paths(&pages, workspace_dir);
        assert_eq!(paths.len(), 2);
        assert_eq!(
            paths[0].0,
            PathBuf::from("/workspace/notes/_attachments/doc.pdf")
        );
        assert_eq!(paths[0].1, PathBuf::from("notes/_attachments/doc.pdf"));
        // Diaryx link format with root-relative path
        assert_eq!(paths[1].0, PathBuf::from("/workspace/public/icon.svg"));
        assert_eq!(paths[1].1, PathBuf::from("public/icon.svg"));
    }

    #[cfg(feature = "markdown")]
    #[test]
    fn test_publish_copies_attachments() {
        let fs = crate::fs::InMemoryFileSystem::new();

        // Create workspace structure
        let workspace_dir = Path::new("/workspace");
        let workspace_root = workspace_dir.join("README.md");
        fs.create_dir_all(workspace_dir).unwrap();
        fs.create_dir_all(&workspace_dir.join("_attachments"))
            .unwrap();
        fs.create_dir_all(&workspace_dir.join("public")).unwrap();
        fs.write_file(
            &workspace_root,
            "---\ntitle: Test Site\ncontents: []\nattachments:\n  - '[Icon](/public/icon.svg)'\n---\n\n![photo](_attachments/image.png)\n\n<img src=\"public/banner.jpg\" alt=\"banner\">\n",
        )
        .unwrap();
        fs.write_binary(
            &workspace_dir.join("_attachments/image.png"),
            b"fake-png-data",
        )
        .unwrap();
        fs.write_binary(&workspace_dir.join("public/icon.svg"), b"<svg>icon</svg>")
            .unwrap();
        fs.write_binary(&workspace_dir.join("public/banner.jpg"), b"fake-jpg-data")
            .unwrap();

        let async_fs = crate::fs::SyncToAsyncFs::new(fs.clone());
        let publisher = Publisher::new(async_fs);
        let dest = Path::new("/output");

        // Publish with copy_attachments: true
        let options = PublishOptions {
            copy_attachments: true,
            force: true,
            ..Default::default()
        };
        let result =
            futures_lite::future::block_on(publisher.publish(&workspace_root, dest, &options))
                .unwrap();
        assert_eq!(result.attachments_copied, 3);
        let copied = fs
            .read_binary(&dest.join("_attachments/image.png"))
            .unwrap();
        assert_eq!(copied, b"fake-png-data");
        let copied_icon = fs.read_binary(&dest.join("public/icon.svg")).unwrap();
        assert_eq!(copied_icon, b"<svg>icon</svg>");
        let copied_banner = fs.read_binary(&dest.join("public/banner.jpg")).unwrap();
        assert_eq!(copied_banner, b"fake-jpg-data");

        // Publish with copy_attachments: false
        let dest2 = Path::new("/output2");
        let options2 = PublishOptions {
            copy_attachments: false,
            force: true,
            ..Default::default()
        };
        let result2 =
            futures_lite::future::block_on(publisher.publish(&workspace_root, dest2, &options2))
                .unwrap();
        assert_eq!(result2.attachments_copied, 0);
        assert!(
            fs.read_binary(&dest2.join("_attachments/image.png"))
                .is_err()
        );
    }

    #[cfg(feature = "markdown")]
    #[test]
    fn test_preprocess_highlight_default_color() {
        let result = preprocess_custom_syntax("==text==");
        assert!(result.contains(r#"class="highlight-mark highlight-yellow""#));
        assert!(result.contains(">text</mark>"));
    }

    #[cfg(feature = "markdown")]
    #[test]
    fn test_preprocess_highlight_with_color() {
        let result = preprocess_custom_syntax("=={red}text==");
        assert!(result.contains(r#"class="highlight-mark highlight-red""#));
        assert!(result.contains(">text</mark>"));
    }

    #[cfg(feature = "markdown")]
    #[test]
    fn test_preprocess_highlight_invalid_color() {
        let input = "=={neon}text==";
        let result = preprocess_custom_syntax(input);
        assert_eq!(result, input);
    }

    #[cfg(feature = "markdown")]
    #[test]
    fn test_preprocess_spoiler() {
        let result = preprocess_custom_syntax("||secret||");
        assert!(result.contains(r#"class="spoiler-mark spoiler-hidden""#));
        assert!(result.contains(">secret</span>"));
    }

    #[cfg(feature = "markdown")]
    #[test]
    fn test_preprocess_skips_fenced_code_block() {
        let input = "```\n==text== ||secret||\n```";
        let result = preprocess_custom_syntax(input);
        assert_eq!(result, input);
    }

    #[cfg(feature = "markdown")]
    #[test]
    fn test_preprocess_skips_inline_code() {
        let input = "`==text==`";
        let result = preprocess_custom_syntax(input);
        assert_eq!(result, input);
    }

    #[cfg(feature = "markdown")]
    #[test]
    fn test_preprocess_empty_highlight_ignored() {
        let input = "====";
        let result = preprocess_custom_syntax(input);
        assert_eq!(result, input);
    }

    #[cfg(feature = "markdown")]
    #[test]
    fn test_preprocess_empty_spoiler_ignored() {
        let input = "||||";
        let result = preprocess_custom_syntax(input);
        assert_eq!(result, input);
    }

    #[cfg(feature = "markdown")]
    #[test]
    fn test_markdown_to_html_footnotes() {
        let fs = crate::fs::SyncToAsyncFs::new(crate::fs::RealFileSystem);
        let publisher = Publisher::new(fs);
        let md = "Here is a footnote[^1].\n\n[^1]: This is the footnote content.";
        let html = publisher.markdown_to_html(md);
        assert!(
            html.contains("footnote"),
            "Footnote HTML not found: {}",
            html
        );
    }

    #[cfg(feature = "markdown")]
    #[test]
    fn test_preprocess_highlight_html_escapes_content() {
        let result = preprocess_custom_syntax("==<b>bold</b>==");
        assert!(result.contains("&lt;b&gt;bold&lt;/b&gt;"));
        assert!(!result.contains("<b>bold</b>"));
    }

    #[cfg(feature = "markdown")]
    #[test]
    fn test_markdown_to_html_colored_highlight() {
        let fs = crate::fs::SyncToAsyncFs::new(crate::fs::RealFileSystem);
        let publisher = Publisher::new(fs);
        let md = "This is =={red}important== text.";
        let html = publisher.markdown_to_html(md);
        eprintln!("Colored highlight HTML: {}", html);
        assert!(
            html.contains("highlight-red"),
            "Expected highlight-red class in: {}",
            html
        );
    }
}
