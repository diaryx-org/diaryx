//! Publisher — format-agnostic workspace publish orchestrator.
//!
//! `Publisher` collects workspace files, resolves navigation, renders body
//! templates, and delegates all format-specific operations (body conversion,
//! link rewriting, page wrapping, static assets) to a [`PublishFormat`] impl.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use diaryx_core::error::{DiaryxError, Result};
use diaryx_core::export::{ExportPlan, Exporter};
use diaryx_core::frontmatter;
use diaryx_core::fs::AsyncFileSystem;
use diaryx_core::link_parser;
use diaryx_core::visibility;
use diaryx_core::workspace::Workspace;
use diaryx_render::nav::{build_site_nav_tree, nav_for_page};

use super::body_renderer::BodyRenderer;
use super::publish_format::PublishFormat;
use super::types::{NavLink, PublishOptions, PublishResult, PublishedPage};

/// A rendered file ready for upload or writing.
#[derive(Debug, Clone)]
pub struct RenderedFile {
    /// Output path relative to the publish root (e.g. `"index.html"`, `"style.css"`).
    pub path: String,
    /// File content as bytes.
    pub content: Vec<u8>,
    /// MIME type (e.g. `"text/html"`, `"text/css"`).
    pub mime_type: String,
    /// Source file's ARK blade (frontmatter `id`) when this rendition is a
    /// content page; `None` for assets (CSS, feeds, nav). Carried so publish
    /// can register the ARK for the page's canonical object.
    pub file_ark: Option<String>,
    /// The page's audience-scoped markdown source, for content pages. Uploaded
    /// as a sibling object so the server can resolve `?content`/`?json`.
    pub source_markdown: Option<String>,
    /// The page's sanitized workspace-relative source path (e.g. `"Welcome.md"`,
    /// `"notes/post.md"`), for content pages. Server-side rendering keys the
    /// uploaded source by this path so frontmatter `contents`/`part_of` links
    /// (which reference workspace paths) resolve correctly; `None` for assets.
    pub source_rel_path: Option<String>,
    /// `true` when this rendition is the workspace root index page.
    pub is_index: bool,
}

/// Top-level frontmatter keys stripped from the markdown source sibling before
/// it is uploaded for ARK Layer 2 resolution (`?content`/`?json`/`?info`). The
/// source is served publicly to anyone who can resolve the ARK, so internal
/// publishing config must not leak — notably `plugins` (which carries
/// `plugins.diaryx.publish` audience/access settings) and the audience
/// definitions. Author-facing metadata (title, description, author, dates,
/// `id`, etc.) is preserved.
const SOURCE_SIBLING_FRONTMATTER_DENYLIST: &[&str] =
    &["plugins", "audiences", "audiences_migrated"];

/// Strip the [`SOURCE_SIBLING_FRONTMATTER_DENYLIST`] keys from a frontmatter map.
fn strip_sensitive_frontmatter(
    frontmatter: &indexmap::IndexMap<String, diaryx_core::yaml::Value>,
) -> indexmap::IndexMap<String, diaryx_core::yaml::Value> {
    let mut filtered = frontmatter.clone();
    for key in SOURCE_SIBLING_FRONTMATTER_DENYLIST {
        filtered.shift_remove(*key);
    }
    filtered
}

const PUBLISHED_HTML_ATTACHMENT_BRIDGE_MARKER: &str = "data-diaryx-published-html-bridge";
const PUBLISHED_HTML_ATTACHMENT_BRIDGE: &str = r#"<script data-diaryx-published-html-bridge>
(() => {
    if (window.__diaryxPublishedHtmlBridgeInstalled) return;
    window.__diaryxPublishedHtmlBridgeInstalled = true;

    function postHeight() {
        const body = document.body;
        const root = document.documentElement;
        const height = Math.max(
            body ? body.scrollHeight : 0,
            body ? body.offsetHeight : 0,
            root ? root.scrollHeight : 0,
            root ? root.offsetHeight : 0,
        );
        if (height > 0) {
            window.parent.postMessage({ type: "diaryx-html-attachment-size", height }, "*");
        }
    }

    function schedulePost() {
        requestAnimationFrame(() => {
            postHeight();
            setTimeout(postHeight, 60);
        });
    }

    window.addEventListener("message", (event) => {
        if (event.data && event.data.type === "diaryx-html-attachment-measure") {
            schedulePost();
        }
    });

    window.addEventListener("load", schedulePost);
    window.addEventListener("resize", schedulePost);

    if (typeof ResizeObserver !== "undefined") {
        const observer = new ResizeObserver(schedulePost);
        if (document.documentElement) observer.observe(document.documentElement);
        if (document.body) observer.observe(document.body);
    }

    if (document.fonts && typeof document.fonts.ready?.then === "function") {
        document.fonts.ready.then(schedulePost).catch(() => {});
    }

    schedulePost();
})();
</script>"#;

pub fn prepare_published_attachment_bytes(path: &Path, bytes: &[u8]) -> Vec<u8> {
    let is_html = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| matches!(ext.to_ascii_lowercase().as_str(), "html" | "htm"))
        .unwrap_or(false);
    if !is_html {
        return bytes.to_vec();
    }

    let html = match String::from_utf8(bytes.to_vec()) {
        Ok(html) => html,
        Err(_) => return bytes.to_vec(),
    };
    if html.contains(PUBLISHED_HTML_ATTACHMENT_BRIDGE_MARKER) {
        return html.into_bytes();
    }

    if let Some(index) = html.rfind("</body>") {
        let mut injected =
            String::with_capacity(html.len() + PUBLISHED_HTML_ATTACHMENT_BRIDGE.len());
        injected.push_str(&html[..index]);
        injected.push_str(PUBLISHED_HTML_ATTACHMENT_BRIDGE);
        injected.push_str(&html[index..]);
        return injected.into_bytes();
    }

    if let Some(index) = html.rfind("</html>") {
        let mut injected =
            String::with_capacity(html.len() + PUBLISHED_HTML_ATTACHMENT_BRIDGE.len());
        injected.push_str(&html[..index]);
        injected.push_str(PUBLISHED_HTML_ATTACHMENT_BRIDGE);
        injected.push_str(&html[index..]);
        return injected.into_bytes();
    }

    let mut injected = String::with_capacity(html.len() + PUBLISHED_HTML_ATTACHMENT_BRIDGE.len());
    injected.push_str(&html);
    injected.push_str(PUBLISHED_HTML_ATTACHMENT_BRIDGE);
    injected.into_bytes()
}

/// Format-agnostic workspace publisher (async-first).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub struct Publisher<'a, FS: AsyncFileSystem> {
    fs: FS,
    body_renderer: &'a dyn BodyRenderer,
    format: &'a dyn PublishFormat,
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
impl<'a, FS: AsyncFileSystem + Clone> Publisher<'a, FS> {
    /// Create a new publisher with the given format.
    pub fn new(fs: FS, body_renderer: &'a dyn BodyRenderer, format: &'a dyn PublishFormat) -> Self {
        Self {
            fs,
            body_renderer,
            format,
        }
    }

    /// Render all workspace files to memory without writing to the filesystem.
    ///
    /// This method is available on all targets (including WASM).
    /// Collect and process pages for an audience without rendering full HTML wrappers.
    ///
    /// Returns `PublishedPage` objects with `rendered_body` containing just the
    /// converted markdown body (no page chrome, nav, metadata pills, etc.).
    /// Useful for email digest rendering where the email format provides its own wrapper.
    pub async fn collect_pages(
        &self,
        workspace_root: &Path,
        options: &PublishOptions,
    ) -> Result<Vec<PublishedPage>> {
        let pages = if let Some(ref audience) = options.audience {
            self.collect_with_audience(
                workspace_root,
                Path::new("/tmp/render"),
                audience,
                options.default_audience.as_deref(),
            )
            .await?
        } else {
            self.collect_all(workspace_root).await?
        };
        Ok(pages)
    }

    pub async fn render(
        &self,
        workspace_root: &Path,
        options: &PublishOptions,
    ) -> Result<Vec<RenderedFile>> {
        let pages = if let Some(ref audience) = options.audience {
            self.collect_with_audience(
                workspace_root,
                // dummy destination — not used for rendering
                Path::new("/tmp/render"),
                audience,
                options.default_audience.as_deref(),
            )
            .await?
        } else {
            self.collect_all(workspace_root).await?
        };

        if pages.is_empty() {
            return Ok(vec![]);
        }

        let site_title = options.title.clone().unwrap_or_else(|| {
            pages
                .first()
                .map(|p| p.title.clone())
                .unwrap_or_else(|| "Journal".to_string())
        });

        let workspace_dir = workspace_root.parent().unwrap_or(workspace_root);
        let nav_tree = build_site_nav_tree(&pages);
        let mut rendered_files = Vec::new();

        for page in &pages {
            let nav = nav_for_page(&nav_tree, &page.dest_filename, &pages);
            let seo_meta = self.format.render_seo_meta(page, &site_title, options);
            let feed_links = self.format.render_feed_links(page);
            let rendered = self.format.render_page_with_context(
                page,
                &site_title,
                false,
                &nav,
                &seo_meta,
                &feed_links,
            );

            let mime_type = match self.format.output_extension() {
                "html" => "text/html",
                "xml" => "application/xml",
                _ => "text/plain",
            };

            rendered_files.push(RenderedFile {
                path: page.dest_filename.clone(),
                content: rendered.into_bytes(),
                mime_type: mime_type.to_string(),
                file_ark: page.file_ark.clone(),
                source_markdown: Some(page.source_markdown.clone()),
                source_rel_path: Some(self.source_rel_path(page, workspace_dir)),
                is_index: page.is_root,
            });
        }

        // Supplementary files (sitemap, feeds, robots.txt)
        for (filename, content) in self.format.supplementary_files(&pages, options) {
            let mime_type = if filename.ends_with(".xml") {
                "application/xml"
            } else if filename.ends_with(".txt") {
                "text/plain"
            } else {
                "application/octet-stream"
            };
            rendered_files.push(RenderedFile {
                path: filename,
                content,
                mime_type: mime_type.to_string(),
                file_ark: None,
                source_markdown: None,
                source_rel_path: None,
                is_index: false,
            });
        }

        // Static assets (CSS, etc.)
        for (filename, content) in self.format.static_assets() {
            let mime_type = if filename.ends_with(".css") {
                "text/css"
            } else if filename.ends_with(".js") {
                "application/javascript"
            } else {
                "application/octet-stream"
            };
            rendered_files.push(RenderedFile {
                path: filename,
                content,
                mime_type: mime_type.to_string(),
                file_ark: None,
                source_markdown: None,
                source_rel_path: None,
                is_index: false,
            });
        }

        Ok(rendered_files)
    }

    /// Render workspace files and collect attachment paths in one pass.
    ///
    /// Returns `(rendered_files, attachment_pairs)` where each attachment pair
    /// is `(source_absolute_path, dest_relative_path)`.
    pub async fn render_with_attachments(
        &self,
        workspace_root: &Path,
        options: &PublishOptions,
    ) -> Result<(Vec<RenderedFile>, Vec<(PathBuf, PathBuf)>)> {
        let pages = if let Some(ref audience) = options.audience {
            self.collect_with_audience(
                workspace_root,
                Path::new("/tmp/render"),
                audience,
                options.default_audience.as_deref(),
            )
            .await?
        } else {
            self.collect_all(workspace_root).await?
        };

        if pages.is_empty() {
            return Ok((vec![], vec![]));
        }

        let workspace_dir = workspace_root.parent().unwrap_or(workspace_root);
        let attachment_paths = Self::collect_attachment_paths(&pages, workspace_dir);

        let site_title = options.title.clone().unwrap_or_else(|| {
            pages
                .first()
                .map(|p| p.title.clone())
                .unwrap_or_else(|| "Journal".to_string())
        });

        let nav_tree = build_site_nav_tree(&pages);
        let mut rendered_files = Vec::new();

        for page in &pages {
            let nav = nav_for_page(&nav_tree, &page.dest_filename, &pages);
            let seo_meta = self.format.render_seo_meta(page, &site_title, options);
            let feed_links = self.format.render_feed_links(page);
            let rendered = self.format.render_page_with_context(
                page,
                &site_title,
                false,
                &nav,
                &seo_meta,
                &feed_links,
            );

            let mime_type = match self.format.output_extension() {
                "html" => "text/html",
                "xml" => "application/xml",
                _ => "text/plain",
            };

            rendered_files.push(RenderedFile {
                path: page.dest_filename.clone(),
                content: rendered.into_bytes(),
                mime_type: mime_type.to_string(),
                file_ark: page.file_ark.clone(),
                source_markdown: Some(page.source_markdown.clone()),
                source_rel_path: Some(self.source_rel_path(page, workspace_dir)),
                is_index: page.is_root,
            });
        }

        for (filename, content) in self.format.supplementary_files(&pages, options) {
            let mime_type = if filename.ends_with(".xml") {
                "application/xml"
            } else if filename.ends_with(".txt") {
                "text/plain"
            } else {
                "application/octet-stream"
            };
            rendered_files.push(RenderedFile {
                path: filename,
                content,
                mime_type: mime_type.to_string(),
                file_ark: None,
                source_markdown: None,
                source_rel_path: None,
                is_index: false,
            });
        }

        for (filename, content) in self.format.static_assets() {
            let mime_type = if filename.ends_with(".css") {
                "text/css"
            } else if filename.ends_with(".js") {
                "application/javascript"
            } else {
                "application/octet-stream"
            };
            rendered_files.push(RenderedFile {
                path: filename,
                content,
                mime_type: mime_type.to_string(),
                file_ark: None,
                source_markdown: None,
                source_rel_path: None,
                is_index: false,
            });
        }

        Ok((rendered_files, attachment_paths))
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
            self.collect_with_audience(
                workspace_root,
                destination,
                audience,
                options.default_audience.as_deref(),
            )
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
                match self.fs.read(src).await {
                    Ok(bytes) => {
                        let prepared = prepare_published_attachment_bytes(dest_rel, &bytes);
                        self.fs.write(&dest, &prepared).await?;
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

        let index_filename = format!("index.{}", self.format.output_extension());

        // First pass: assign filenames
        for (idx, file_path) in files.iter().enumerate() {
            let filename = if idx == 0 {
                index_filename.clone()
            } else {
                self.format.output_filename(file_path, workspace_dir)
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
        default_audience: Option<&str>,
    ) -> Result<Vec<PublishedPage>> {
        let exporter = Exporter::new(self.fs.clone());
        let plan = exporter
            .plan_export(workspace_root, audience, destination, default_audience)
            .await?;

        let workspace_dir = workspace_root.parent().unwrap_or(workspace_root);
        let mut pages = Vec::new();
        let mut path_to_filename: HashMap<PathBuf, String> = HashMap::new();
        let index_filename = format!("index.{}", self.format.output_extension());

        // Ensure the workspace root is first (it becomes the index file).
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
                index_filename.clone()
            } else {
                self.format
                    .output_filename(&export_file.source_path, workspace_dir)
            };
            path_to_filename.insert(export_file.source_path.clone(), filename);
        }

        // Second pass: process files (with audience-aware template rendering)
        for (idx, export_file) in included.iter().enumerate() {
            if let Some(page) = self
                .process_file_with_audience(
                    &export_file.source_path,
                    idx == 0,
                    &path_to_filename,
                    workspace_root,
                    Some(audience),
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
            .map(|f| self.format.output_filename(&f.source_path, workspace_dir))
            .collect();

        // Also include index file for the root
        let mut allowed = included_filenames;
        allowed.insert(format!("index.{}", self.format.output_extension()));

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
        self.process_file_with_audience(path, is_root, path_to_filename, workspace_root, None)
            .await
    }

    /// Process a single file into a PublishedPage, optionally with a target audience
    /// for audience-aware template rendering.
    async fn process_file_with_audience(
        &self,
        path: &Path,
        is_root: bool,
        path_to_filename: &HashMap<PathBuf, String>,
        workspace_root: &Path,
        _target_audience: Option<&str>,
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
            .unwrap_or_else(|| self.format.output_filename(path, workspace_dir));

        // Build contents links
        let contents_links = self
            .build_contents_links(&parsed.frontmatter, path, path_to_filename, workspace_dir)
            .await;

        // Build parent link
        let parent_link = self
            .build_parent_link(&parsed.frontmatter, path, path_to_filename, workspace_dir)
            .await;

        // Apply audience visibility filtering before any template rendering.
        let visibility_filtered_body = match _target_audience {
            Some(audience) => visibility::filter_body_for_audience(&parsed.body, audience),
            None => visibility::strip_visibility_directives(&parsed.body),
        };

        // The audience-scoped markdown source the server stores for Layer 2
        // resolution (?content/?json/?info). This is served publicly, so the
        // frontmatter has sensitive internal keys (publishing config, audience
        // definitions) stripped first; the body has had this audience's
        // visibility filtering applied.
        let source_frontmatter = strip_sensitive_frontmatter(&parsed.frontmatter);
        let source_markdown =
            frontmatter::serialize(&source_frontmatter, &visibility_filtered_body)
                .unwrap_or_else(|_| visibility_filtered_body.clone());

        // Render body templates (if any) before markdown-to-HTML conversion
        let rendered_body = if self.body_renderer.has_templates(&visibility_filtered_body) {
            self.body_renderer
                .render_body(
                    &visibility_filtered_body,
                    &parsed.frontmatter,
                    path,
                    Some(workspace_root),
                    _target_audience,
                )
                .unwrap_or_else(|_| visibility_filtered_body.clone())
        } else {
            visibility_filtered_body
        };

        // Convert body to output format and transform links
        let preprocessed = self.format.preprocess_body(&rendered_body);
        let converted = self.format.convert_body(&preprocessed);
        let rendered = self.format.transform_links(
            &converted,
            path,
            path_to_filename,
            workspace_dir,
            &dest_filename,
        );

        let nav_title = frontmatter::get_string(&parsed.frontmatter, "nav_title").map(String::from);
        let nav_order = parsed.frontmatter.get("nav_order").and_then(|v| match v {
            diaryx_core::yaml::Value::Int(i) => Some(*i as i32),
            diaryx_core::yaml::Value::Float(f) => Some(*f as i32),
            diaryx_core::yaml::Value::String(s) => s.parse::<i32>().ok(),
            _ => None,
        });
        let hide_from_nav = parsed
            .frontmatter
            .get("hide_from_nav")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let hide_from_feed = parsed
            .frontmatter
            .get("hide_from_feed")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let description =
            frontmatter::get_string(&parsed.frontmatter, "description").map(String::from);
        let author = frontmatter::get_string(&parsed.frontmatter, "author").map(String::from);
        let created = frontmatter::get_string(&parsed.frontmatter, "created").map(String::from);
        let updated = frontmatter::get_string(&parsed.frontmatter, "updated").map(String::from);
        let attachments = frontmatter::get_string_array(&parsed.frontmatter, "attachments");

        Ok(Some(PublishedPage {
            source_path: path.to_path_buf(),
            dest_filename,
            title,
            rendered_body: rendered,
            markdown_body: rendered_body,
            contents_links,
            parent_link,
            is_root,
            description,
            author,
            created,
            updated,
            attachments,
            nav_title,
            nav_order,
            hide_from_nav,
            hide_from_feed,
            file_ark: frontmatter::get_string(&parsed.frontmatter, "id").map(String::from),
            source_markdown,
        }))
    }

    /// Build navigation links from contents property
    async fn build_contents_links(
        &self,
        fm: &indexmap::IndexMap<String, diaryx_core::yaml::Value>,
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
                .unwrap_or_else(|| self.format.output_filename(&child_path, workspace_dir));

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
        fm: &indexmap::IndexMap<String, diaryx_core::yaml::Value>,
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
            .unwrap_or_else(|| self.format.output_filename(&parent_path, workspace_dir));

        let title = self
            .get_title_from_file(&parent_path)
            .await
            .or_else(|| parsed.title.clone())
            .unwrap_or_else(|| self.filename_to_title(&canonical));

        Some(NavLink { href, title })
    }

    /// Sanitized workspace-relative `.md` source path for a page (e.g.
    /// `"Welcome.md"`, `"notes/post.md"`). Server-side rendering keys the
    /// uploaded source by this path so frontmatter links resolve correctly.
    fn source_rel_path(&self, page: &PublishedPage, workspace_dir: &Path) -> String {
        let dest = self
            .format
            .output_filename(&page.source_path, workspace_dir);
        Path::new(&dest)
            .with_extension("md")
            .to_string_lossy()
            .into_owned()
    }

    /// Get title from a file's frontmatter
    async fn get_title_from_file(&self, path: &Path) -> Option<String> {
        let content = self.fs.read_to_string(path).await.ok()?;
        let parsed = frontmatter::parse_or_empty(&content).ok()?;
        frontmatter::get_string(&parsed.frontmatter, "title").map(String::from)
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

    /// Collect non-markdown file paths referenced by published pages.
    ///
    /// Scans each page's markdown body for local file references (images,
    /// PDFs, etc.) and the frontmatter `attachments` list. Returns
    /// deduplicated pairs of `(source_absolute_path, dest_relative_path)`.
    /// Markdown files are excluded since they become HTML pages.
    pub fn collect_attachment_paths(
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

            // Check attachments list
            for s in &page.attachments {
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

        let index_filename = format!("index.{}", self.format.output_extension());

        // Build site-wide navigation tree
        let nav_tree = build_site_nav_tree(pages);

        for page in pages {
            let nav = nav_for_page(&nav_tree, &page.dest_filename, pages);
            let seo_meta = self.format.render_seo_meta(page, &site_title, options);
            let feed_links = self.format.render_feed_links(page);
            let rendered = self.format.render_page_with_context(
                page,
                &site_title,
                false,
                &nav,
                &seo_meta,
                &feed_links,
            );
            let dest_path = destination.join(&page.dest_filename);

            // Create subdirectories as needed (dest_filename may contain paths)
            if let Some(parent) = dest_path.parent() {
                self.fs.create_dir_all(parent).await?;
            }

            self.fs.write(&dest_path, rendered.as_bytes()).await?;

            // Write root page under its original filename too, so both
            // localhost/ and localhost/readme.html (or similar) work
            if page.is_root && page.dest_filename == index_filename {
                let ext = self.format.output_extension();
                let original_filename = page
                    .source_path
                    .with_extension(ext)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned());
                if let Some(name) = original_filename
                    && name != index_filename
                {
                    let alias_path = destination.join(&name);
                    self.fs.write(&alias_path, rendered.as_bytes()).await?;
                }
            }
        }

        // Write supplementary files (sitemap, feeds, robots.txt)
        for (filename, content) in self.format.supplementary_files(pages, options) {
            let supp_path = destination.join(filename);
            self.fs.write(&supp_path, &content).await?;
        }

        // Write static assets (e.g., CSS for HTML format)
        for (filename, content) in self.format.static_assets() {
            let asset_path = destination.join(filename);
            self.fs.write(&asset_path, &content).await?;
        }

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

        let rendered = self.format.render_single_document(pages, &site_title);

        // Ensure parent directory exists
        if let Some(parent) = destination.parent() {
            self.fs.create_dir_all(parent).await?;
        }

        self.fs.write(destination, rendered.as_bytes()).await?;

        Ok(())
    }
}

/// Extract local file reference paths from markdown text.
///
/// Finds references inside markdown link/image syntax `[...](...)`
/// and HTML attributes `src="..."` / `href="..."` / `srcset="..."`.
/// Excludes external
/// URLs, anchors, and data/javascript URIs.
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

    // Find paths in HTML srcset attributes: srcset="path 1x, other 2x"
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

/// Returns true if a path looks like a local file reference (not an external
/// URL, anchor, or special URI scheme).
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

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_sensitive_frontmatter_removes_denylisted_keys() {
        use diaryx_core::yaml::Value;

        let mut fm = indexmap::IndexMap::new();
        fm.insert("title".to_string(), Value::String("Hello".into()));
        fm.insert("id".to_string(), Value::String("bcdfgr".into()));
        fm.insert("plugins".to_string(), Value::String("secret-config".into()));
        fm.insert(
            "audiences".to_string(),
            Value::String("family,public".into()),
        );
        fm.insert("audiences_migrated".to_string(), Value::Bool(true));

        let stripped = strip_sensitive_frontmatter(&fm);

        // Author-facing keys survive.
        assert!(stripped.contains_key("title"));
        assert!(stripped.contains_key("id"));
        // Sensitive keys are gone.
        assert!(!stripped.contains_key("plugins"));
        assert!(!stripped.contains_key("audiences"));
        assert!(!stripped.contains_key("audiences_migrated"));
    }

    #[test]
    fn test_source_markdown_strips_sensitive_frontmatter() {
        use super::super::html_format::HtmlFormat;
        use diaryx_core::fs::FileSystem;

        let fs = diaryx_core::fs::InMemoryFileSystem::new();
        let workspace_dir = Path::new("/workspace");
        let workspace_root = workspace_dir.join("README.md");
        fs.create_dir_all(workspace_dir).unwrap();
        fs.write(
            &workspace_root,
            concat!(
                "---\n",
                "title: My Journal\n",
                "id: bcdfgr\n",
                "author: Adam\n",
                "audiences:\n",
                "  public: []\n",
                "audiences_migrated: true\n",
                "plugins:\n",
                "  diaryx:\n",
                "    publish:\n",
                "      audience: family\n",
                "      access: private\n",
                "contents: []\n",
                "---\n\n",
                "Hello world\n",
            )
            .as_bytes(),
        )
        .unwrap();

        let async_fs = diaryx_core::fs::SyncToAsyncFs::new(fs);
        let renderer = super::super::body_renderer::NoopBodyRenderer;
        let format = HtmlFormat::new();
        let publisher = Publisher::new(async_fs, &renderer, &format);

        let pages = futures_lite::future::block_on(
            publisher.collect_pages(&workspace_root, &PublishOptions::default()),
        )
        .unwrap();

        let source = &pages[0].source_markdown;
        // Sensitive internal keys must not appear in the served source sibling.
        assert!(
            !source.contains("plugins"),
            "source leaked `plugins`: {source}"
        );
        assert!(
            !source.contains("audiences"),
            "source leaked `audiences`/`audiences_migrated`: {source}"
        );
        assert!(
            !source.contains("access"),
            "source leaked publish access config: {source}"
        );
        // Author-facing metadata and body are preserved.
        assert!(source.contains("title: My Journal"));
        assert!(source.contains("id: bcdfgr"));
        assert!(source.contains("author: Adam"));
        assert!(source.contains("Hello world"));
    }

    #[test]
    fn test_transform_links_no_corruption() {
        use super::super::html_format::HtmlFormat;

        let format = HtmlFormat::new();
        let workspace_dir = Path::new("/tmp/workspace");
        let current_path = workspace_dir.join("family.md");
        let mut path_to_filename = HashMap::new();
        path_to_filename.insert(
            workspace_dir.join("Message for my family.md"),
            "index.html".to_string(),
        );
        path_to_filename.insert(workspace_dir.join("family.md"), "family.html".to_string());

        // Simulate comrak output for: [Click me!](/family.md)
        let html1 = r#"<p><a href="/family.md">Click me!</a></p>"#;
        let result1 = format.transform_links(
            html1,
            &workspace_dir.join("Message for my family.md"),
            &path_to_filename,
            workspace_dir,
            "index.html",
        );
        assert!(
            result1.contains(">Click me!</a></p>"),
            "Link text corrupted: {}",
            result1
        );

        // Simulate comrak output for: [← Go back](</Message for my family.md>)
        let html2 = r#"<h1>Hooray, you made it!</h1>
<p>That's all folks!</p>
<p><a href="/Message%20for%20my%20family.md">← Go back</a></p>"#;
        let result2 = format.transform_links(
            html2,
            &current_path,
            &path_to_filename,
            workspace_dir,
            "family.html",
        );
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
    fn test_extract_local_file_refs_html_srcset() {
        let md = r#"<picture><source media="(prefers-color-scheme: dark)" srcset="apps/web/public/icon-dark.png"><source media="(prefers-color-scheme: light)" srcset="apps/web/public/icon.png 1x, apps/web/public/icon@2x.png 2x"><img alt="Diaryx icon" src="apps/web/public/icon.png" width="128"></picture>"#;
        let refs = extract_local_file_refs(md);
        assert_eq!(refs.len(), 4);
        assert_eq!(refs[0], "apps/web/public/icon.png");
        assert_eq!(refs[1], "apps/web/public/icon-dark.png");
        assert_eq!(refs[2], "apps/web/public/icon.png");
        assert_eq!(refs[3], "apps/web/public/icon@2x.png");
    }

    #[test]
    fn test_extract_local_file_refs_skips_external_and_anchors() {
        let md = "[link](https://example.com)\n[anchor](#heading)\n[mail](mailto:a@b.com)\nplain text (no file ref)";
        let refs = extract_local_file_refs(md);
        assert!(refs.is_empty());
    }

    #[test]
    fn test_extract_local_file_refs_skips_md_links() {
        let md = "[sibling](./other.md)";
        let refs = extract_local_file_refs(md);
        assert!(refs.contains(&"./other.md".to_string()));
    }

    #[test]
    fn test_collect_attachment_paths_deduplicates() {
        let workspace_dir = Path::new("/workspace");
        let pages = vec![PublishedPage {
            source_path: PathBuf::from("/workspace/README.md"),
            dest_filename: "index.html".to_string(),
            title: "Root".to_string(),
            rendered_body: String::new(),
            markdown_body: "![img](_attachments/a.png)\n![img2](_attachments/a.png)".to_string(),
            contents_links: vec![],
            parent_link: None,
            is_root: true,
            description: None,
            author: None,
            created: None,
            updated: None,
            attachments: vec![],
            nav_title: None,
            nav_order: None,
            hide_from_nav: false,
            hide_from_feed: false,
            file_ark: None,
            source_markdown: String::new(),
        }];
        let paths = Publisher::<diaryx_core::fs::SyncToAsyncFs<diaryx_core::fs::InMemoryFileSystem>>::collect_attachment_paths(&pages, workspace_dir);
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].0, PathBuf::from("/workspace/_attachments/a.png"));
        assert_eq!(paths[0].1, PathBuf::from("_attachments/a.png"));
    }

    #[test]
    fn test_collect_attachment_paths_from_frontmatter() {
        let workspace_dir = Path::new("/workspace");
        let pages = vec![PublishedPage {
            source_path: PathBuf::from("/workspace/notes/entry.md"),
            dest_filename: "notes/entry.html".to_string(),
            title: "Entry".to_string(),
            rendered_body: String::new(),
            markdown_body: String::new(),
            contents_links: vec![],
            parent_link: None,
            is_root: false,
            description: None,
            author: None,
            created: None,
            updated: None,
            attachments: vec![
                "_attachments/doc.pdf".to_string(),
                "[Icon](/public/icon.svg)".to_string(),
            ],
            nav_title: None,
            nav_order: None,
            hide_from_nav: false,
            hide_from_feed: false,
            file_ark: None,
            source_markdown: String::new(),
        }];
        let paths = Publisher::<diaryx_core::fs::SyncToAsyncFs<diaryx_core::fs::InMemoryFileSystem>>::collect_attachment_paths(&pages, workspace_dir);
        assert_eq!(paths.len(), 2);
        assert_eq!(
            paths[0].0,
            PathBuf::from("/workspace/notes/_attachments/doc.pdf")
        );
        assert_eq!(paths[0].1, PathBuf::from("notes/_attachments/doc.pdf"));
        assert_eq!(paths[1].0, PathBuf::from("/workspace/public/icon.svg"));
        assert_eq!(paths[1].1, PathBuf::from("public/icon.svg"));
    }

    #[test]
    fn test_publish_copies_attachments() {
        use super::super::html_format::HtmlFormat;
        use diaryx_core::fs::FileSystem;

        let fs = diaryx_core::fs::InMemoryFileSystem::new();
        let workspace_dir = Path::new("/workspace");
        let workspace_root = workspace_dir.join("README.md");
        fs.create_dir_all(workspace_dir).unwrap();
        fs.create_dir_all(&workspace_dir.join("_attachments"))
            .unwrap();
        fs.create_dir_all(&workspace_dir.join("public")).unwrap();
        fs.write(
            &workspace_root,
            "---\ntitle: Test Site\ncontents: []\nattachments:\n  - '[Icon](/public/icon.svg)'\n---\n\n![photo](_attachments/image.png)\n\n<img src=\"public/banner.jpg\" alt=\"banner\">\n".as_bytes(),
        )
        .unwrap();
        fs.write(
            &workspace_dir.join("_attachments/image.png"),
            b"fake-png-data",
        )
        .unwrap();
        fs.write(&workspace_dir.join("public/icon.svg"), b"<svg>icon</svg>")
            .unwrap();
        fs.write(&workspace_dir.join("public/banner.jpg"), b"fake-jpg-data")
            .unwrap();

        let async_fs = diaryx_core::fs::SyncToAsyncFs::new(fs.clone());
        let renderer = super::super::body_renderer::NoopBodyRenderer;
        let format = HtmlFormat::new();
        let publisher = Publisher::new(async_fs, &renderer, &format);
        let dest = Path::new("/output");

        let options = PublishOptions {
            copy_attachments: true,
            force: true,
            ..Default::default()
        };
        let result =
            futures_lite::future::block_on(publisher.publish(&workspace_root, dest, &options))
                .unwrap();
        assert_eq!(result.attachments_copied, 3);
        assert_eq!(
            fs.read(&dest.join("_attachments/image.png")).unwrap(),
            b"fake-png-data"
        );
        assert_eq!(
            fs.read(&dest.join("public/icon.svg")).unwrap(),
            b"<svg>icon</svg>"
        );
        assert_eq!(
            fs.read(&dest.join("public/banner.jpg")).unwrap(),
            b"fake-jpg-data"
        );

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
        assert!(fs.read(&dest2.join("_attachments/image.png")).is_err());
    }

    #[test]
    fn test_publish_injects_resize_bridge_into_html_attachments() {
        use super::super::html_format::HtmlFormat;
        use diaryx_core::fs::FileSystem;

        let fs = diaryx_core::fs::InMemoryFileSystem::new();
        let workspace_dir = Path::new("/workspace");
        let workspace_root = workspace_dir.join("README.md");
        fs.create_dir_all(workspace_dir).unwrap();
        fs.create_dir_all(&workspace_dir.join("_attachments"))
            .unwrap();
        fs.write(
            &workspace_root,
            "---\ntitle: Test Site\ncontents: []\n---\n\n![demo](_attachments/demo.html)\n"
                .as_bytes(),
        )
        .unwrap();
        fs.write(
            &workspace_dir.join("_attachments/demo.html"),
            br#"<!doctype html><html><body><main>Demo</main></body></html>"#,
        )
        .unwrap();

        let async_fs = diaryx_core::fs::SyncToAsyncFs::new(fs.clone());
        let renderer = super::super::body_renderer::NoopBodyRenderer;
        let format = HtmlFormat::new();
        let publisher = Publisher::new(async_fs, &renderer, &format);
        let dest = Path::new("/output");

        futures_lite::future::block_on(publisher.publish(
            &workspace_root,
            dest,
            &PublishOptions {
                copy_attachments: true,
                force: true,
                ..Default::default()
            },
        ))
        .unwrap();

        let published =
            String::from_utf8(fs.read(&dest.join("_attachments/demo.html")).unwrap()).unwrap();
        assert!(published.contains("data-diaryx-published-html-bridge"));
        assert!(published.contains("diaryx-html-attachment-size"));
    }

    #[test]
    fn test_prepare_published_attachment_bytes_leaves_non_html_attachments_unchanged() {
        let bytes = b"fake-png-data";
        let prepared =
            prepare_published_attachment_bytes(Path::new("_attachments/demo.png"), bytes);
        assert_eq!(prepared, bytes);
    }

    #[test]
    fn test_render_with_attachments_uses_workspace_directory_for_attachment_sources() {
        use super::super::html_format::HtmlFormat;
        use diaryx_core::fs::FileSystem;

        let fs = diaryx_core::fs::InMemoryFileSystem::new();
        let workspace_dir = Path::new("/workspace");
        let workspace_root = workspace_dir.join("README.md");
        fs.create_dir_all(workspace_dir).unwrap();
        fs.create_dir_all(&workspace_dir.join("_attachments"))
            .unwrap();
        fs.write(
            &workspace_root,
            "---\ntitle: Test Site\ncontents: []\n---\n\n![photo](_attachments/image.png)\n"
                .as_bytes(),
        )
        .unwrap();
        fs.write(
            &workspace_dir.join("_attachments/image.png"),
            b"fake-png-data",
        )
        .unwrap();

        let async_fs = diaryx_core::fs::SyncToAsyncFs::new(fs);
        let renderer = super::super::body_renderer::NoopBodyRenderer;
        let format = HtmlFormat::new();
        let publisher = Publisher::new(async_fs, &renderer, &format);

        let (_rendered, attachments) = futures_lite::future::block_on(
            publisher.render_with_attachments(&workspace_root, &PublishOptions::default()),
        )
        .unwrap();

        assert_eq!(attachments.len(), 1);
        assert_eq!(
            attachments[0].0,
            workspace_dir.join("_attachments/image.png")
        );
    }
}
