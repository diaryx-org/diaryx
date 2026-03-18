//! Publisher — format-agnostic workspace publish orchestrator.
//!
//! `Publisher` collects workspace files, resolves navigation, renders body
//! templates, and delegates all format-specific operations (body conversion,
//! link rewriting, page wrapping, static assets) to a [`PublishFormat`] impl.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::error::{DiaryxError, Result};
use crate::export::{ExportPlan, Exporter};
use crate::frontmatter;
use crate::fs::AsyncFileSystem;
use crate::link_parser;
use crate::workspace::Workspace;

use super::body_renderer::BodyRenderer;
use super::publish_format::PublishFormat;
use super::types::{
    NavLink, PublishOptions, PublishResult, PublishedPage, SiteNavNode, SiteNavigation,
};

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

        // Render body templates (if any) before markdown-to-HTML conversion
        let rendered_body = if self.body_renderer.has_templates(&parsed.body) {
            self.body_renderer
                .render_body(
                    &parsed.body,
                    &parsed.frontmatter,
                    path,
                    Some(workspace_root),
                    _target_audience,
                )
                .unwrap_or_else(|_| parsed.body.clone())
        } else {
            parsed.body.clone()
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
            serde_yaml::Value::Number(n) => n.as_i64().map(|i| i as i32),
            serde_yaml::Value::String(s) => s.parse::<i32>().ok(),
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

        Ok(Some(PublishedPage {
            source_path: path.to_path_buf(),
            dest_filename,
            title,
            rendered_body: rendered,
            markdown_body: rendered_body,
            contents_links,
            parent_link,
            is_root,
            frontmatter: parsed.frontmatter.clone(),
            nav_title,
            nav_order,
            hide_from_nav,
            hide_from_feed,
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
            .unwrap_or_else(|| self.format.output_filename(&parent_path, workspace_dir));

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

            self.fs.write_file(&dest_path, &rendered).await?;

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
                    self.fs.write_file(&alias_path, &rendered).await?;
                }
            }
        }

        // Write supplementary files (sitemap, feeds, robots.txt)
        for (filename, content) in self.format.supplementary_files(pages, options) {
            let supp_path = destination.join(filename);
            self.fs.write_binary(&supp_path, &content).await?;
        }

        // Write static assets (e.g., CSS for HTML format)
        for (filename, content) in self.format.static_assets() {
            let asset_path = destination.join(filename);
            self.fs.write_binary(&asset_path, &content).await?;
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

        self.fs.write_file(destination, &rendered).await?;

        Ok(())
    }
}

/// Build a site navigation tree from all published pages.
///
/// Uses each page's `contents_links` and `parent_link` to build a tree rooted
/// at the page with `is_root == true`. Filters out `hide_from_nav` pages and
/// sorts children by `nav_order` (if present), then by their position in the
/// parent's `contents_links`.
pub fn build_site_nav_tree(pages: &[PublishedPage]) -> Vec<SiteNavNode> {
    // Map dest_filename → page for quick lookup
    let page_map: HashMap<&str, &PublishedPage> = pages
        .iter()
        .map(|p| (p.dest_filename.as_str(), p))
        .collect();

    // Find root page
    let root = match pages.iter().find(|p| p.is_root) {
        Some(r) => r,
        None => return vec![],
    };

    // Build children for a page recursively
    fn build_children(
        page: &PublishedPage,
        page_map: &HashMap<&str, &PublishedPage>,
        depth: usize,
    ) -> Vec<SiteNavNode> {
        if depth >= 3 || page.contents_links.is_empty() {
            return vec![];
        }

        let mut children: Vec<(usize, SiteNavNode)> = Vec::new();

        for (idx, link) in page.contents_links.iter().enumerate() {
            let child_page = page_map.get(link.href.as_str());

            // Skip hidden pages
            if let Some(cp) = child_page {
                if cp.hide_from_nav {
                    continue;
                }
            }

            let title = child_page
                .and_then(|cp| cp.nav_title.as_deref())
                .unwrap_or(&link.title)
                .to_string();

            let sub_children = child_page
                .map(|cp| build_children(cp, page_map, depth + 1))
                .unwrap_or_default();

            let nav_order = child_page.and_then(|cp| cp.nav_order);
            let sort_key = nav_order.unwrap_or(idx as i32);

            children.push((
                sort_key as usize,
                SiteNavNode {
                    title,
                    href: link.href.clone(),
                    is_current: false,
                    is_ancestor_of_current: false,
                    children: sub_children,
                },
            ));
        }

        // Sort by nav_order (encoded in sort_key), stable for equal keys
        children.sort_by_key(|(key, _)| *key);
        children.into_iter().map(|(_, node)| node).collect()
    }

    let root_children = build_children(root, &page_map, 0);

    // Build root node
    let root_title = root.nav_title.as_deref().unwrap_or(&root.title).to_string();

    vec![SiteNavNode {
        title: root_title,
        href: root.dest_filename.clone(),
        is_current: false,
        is_ancestor_of_current: false,
        children: root_children,
    }]
}

/// Build navigation context (tree with current-page marking + breadcrumbs) for a specific page.
pub fn nav_for_page(
    tree: &[SiteNavNode],
    current_dest: &str,
    pages: &[PublishedPage],
) -> SiteNavigation {
    // Deep-clone and mark current + ancestors
    fn mark_current(nodes: &[SiteNavNode], target: &str) -> (Vec<SiteNavNode>, bool) {
        let mut result = Vec::with_capacity(nodes.len());
        let mut found = false;

        for node in nodes {
            let (children, child_found) = mark_current(&node.children, target);
            let is_current = node.href == target;
            let is_ancestor = child_found;

            if is_current || is_ancestor {
                found = true;
            }

            result.push(SiteNavNode {
                title: node.title.clone(),
                href: node.href.clone(),
                is_current,
                is_ancestor_of_current: is_ancestor,
                children,
            });
        }

        (result, found)
    }

    let (marked_tree, _) = mark_current(tree, current_dest);

    // Build breadcrumbs by walking parent_link chain
    let page_map: HashMap<&str, &PublishedPage> = pages
        .iter()
        .map(|p| (p.dest_filename.as_str(), p))
        .collect();

    let mut breadcrumbs = Vec::new();
    if let Some(current_page) = page_map.get(current_dest) {
        // Walk up parent chain
        let mut chain = vec![NavLink {
            href: current_page.dest_filename.clone(),
            title: current_page
                .nav_title
                .clone()
                .unwrap_or_else(|| current_page.title.clone()),
        }];

        let mut visited = HashSet::new();
        visited.insert(current_dest.to_string());

        let mut cursor = current_page.parent_link.as_ref();
        while let Some(parent) = cursor {
            if !visited.insert(parent.href.clone()) {
                break; // cycle guard
            }
            chain.push(NavLink {
                href: parent.href.clone(),
                title: page_map
                    .get(parent.href.as_str())
                    .and_then(|p| p.nav_title.clone())
                    .unwrap_or_else(|| parent.title.clone()),
            });
            cursor = page_map
                .get(parent.href.as_str())
                .and_then(|p| p.parent_link.as_ref());
        }

        chain.reverse();
        breadcrumbs = chain;
    }

    SiteNavigation {
        tree: marked_tree,
        breadcrumbs,
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

/// Decode percent-encoded characters in a URL string (e.g. `%20` → ` `).
#[cfg_attr(not(feature = "html-publish"), allow(dead_code))]
pub(crate) fn percent_decode(input: &str) -> String {
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

#[cfg_attr(not(feature = "html-publish"), allow(dead_code))]
pub(crate) fn hex_val(b: u8) -> Option<u8> {
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

    #[cfg(feature = "html-publish")]
    #[test]
    fn test_transform_links_no_corruption() {
        use super::super::html_format::HtmlFormat;

        let format = HtmlFormat;
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
            frontmatter: indexmap::IndexMap::new(),
            nav_title: None,
            nav_order: None,
            hide_from_nav: false,
            hide_from_feed: false,
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
            rendered_body: String::new(),
            markdown_body: String::new(),
            contents_links: vec![],
            parent_link: None,
            is_root: false,
            frontmatter: fm,
            nav_title: None,
            nav_order: None,
            hide_from_nav: false,
            hide_from_feed: false,
        }];
        let paths = Publisher::<crate::fs::SyncToAsyncFs<crate::fs::InMemoryFileSystem>>::collect_attachment_paths(&pages, workspace_dir);
        assert_eq!(paths.len(), 2);
        assert_eq!(
            paths[0].0,
            PathBuf::from("/workspace/notes/_attachments/doc.pdf")
        );
        assert_eq!(paths[0].1, PathBuf::from("notes/_attachments/doc.pdf"));
        assert_eq!(paths[1].0, PathBuf::from("/workspace/public/icon.svg"));
        assert_eq!(paths[1].1, PathBuf::from("public/icon.svg"));
    }

    #[cfg(feature = "html-publish")]
    #[test]
    fn test_publish_copies_attachments() {
        use super::super::html_format::HtmlFormat;
        use crate::fs::FileSystem;

        let fs = crate::fs::InMemoryFileSystem::new();
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
        let renderer = super::super::body_renderer::NoopBodyRenderer;
        let format = HtmlFormat;
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
            fs.read_binary(&dest.join("_attachments/image.png"))
                .unwrap(),
            b"fake-png-data"
        );
        assert_eq!(
            fs.read_binary(&dest.join("public/icon.svg")).unwrap(),
            b"<svg>icon</svg>"
        );
        assert_eq!(
            fs.read_binary(&dest.join("public/banner.jpg")).unwrap(),
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
        assert!(
            fs.read_binary(&dest2.join("_attachments/image.png"))
                .is_err()
        );
    }

    fn make_page(
        dest: &str,
        title: &str,
        is_root: bool,
        contents: Vec<NavLink>,
        parent: Option<NavLink>,
    ) -> PublishedPage {
        PublishedPage {
            source_path: PathBuf::from(format!("/workspace/{}", dest.replace(".html", ".md"))),
            dest_filename: dest.to_string(),
            title: title.to_string(),
            rendered_body: String::new(),
            markdown_body: String::new(),
            contents_links: contents,
            parent_link: parent,
            is_root,
            frontmatter: indexmap::IndexMap::new(),
            nav_title: None,
            nav_order: None,
            hide_from_nav: false,
            hide_from_feed: false,
        }
    }

    #[test]
    fn test_nav_tree_flat_workspace() {
        let pages = vec![
            make_page(
                "index.html",
                "Home",
                true,
                vec![
                    NavLink {
                        href: "a.html".into(),
                        title: "A".into(),
                    },
                    NavLink {
                        href: "b.html".into(),
                        title: "B".into(),
                    },
                ],
                None,
            ),
            make_page(
                "a.html",
                "A",
                false,
                vec![],
                Some(NavLink {
                    href: "index.html".into(),
                    title: "Home".into(),
                }),
            ),
            make_page(
                "b.html",
                "B",
                false,
                vec![],
                Some(NavLink {
                    href: "index.html".into(),
                    title: "Home".into(),
                }),
            ),
        ];

        let tree = build_site_nav_tree(&pages);
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].title, "Home");
        assert_eq!(tree[0].children.len(), 2);
        assert_eq!(tree[0].children[0].title, "A");
        assert_eq!(tree[0].children[1].title, "B");
    }

    #[test]
    fn test_nav_tree_deep_hierarchy() {
        let pages = vec![
            make_page(
                "index.html",
                "Root",
                true,
                vec![NavLink {
                    href: "parent.html".into(),
                    title: "Parent".into(),
                }],
                None,
            ),
            make_page(
                "parent.html",
                "Parent",
                false,
                vec![NavLink {
                    href: "child.html".into(),
                    title: "Child".into(),
                }],
                Some(NavLink {
                    href: "index.html".into(),
                    title: "Root".into(),
                }),
            ),
            make_page(
                "child.html",
                "Child",
                false,
                vec![NavLink {
                    href: "grandchild.html".into(),
                    title: "Grandchild".into(),
                }],
                Some(NavLink {
                    href: "parent.html".into(),
                    title: "Parent".into(),
                }),
            ),
            make_page(
                "grandchild.html",
                "Grandchild",
                false,
                vec![],
                Some(NavLink {
                    href: "child.html".into(),
                    title: "Child".into(),
                }),
            ),
        ];

        let tree = build_site_nav_tree(&pages);
        assert_eq!(tree[0].children.len(), 1); // Parent
        assert_eq!(tree[0].children[0].children.len(), 1); // Child
        assert_eq!(tree[0].children[0].children[0].children.len(), 1); // Grandchild
        // Depth 3: grandchild's children are empty (max depth reached)
        assert_eq!(
            tree[0].children[0].children[0].children[0].children.len(),
            0
        );
    }

    #[test]
    fn test_nav_tree_hide_from_nav() {
        let mut hidden_page = make_page(
            "hidden.html",
            "Hidden",
            false,
            vec![],
            Some(NavLink {
                href: "index.html".into(),
                title: "Home".into(),
            }),
        );
        hidden_page.hide_from_nav = true;

        let pages = vec![
            make_page(
                "index.html",
                "Home",
                true,
                vec![
                    NavLink {
                        href: "visible.html".into(),
                        title: "Visible".into(),
                    },
                    NavLink {
                        href: "hidden.html".into(),
                        title: "Hidden".into(),
                    },
                ],
                None,
            ),
            make_page(
                "visible.html",
                "Visible",
                false,
                vec![],
                Some(NavLink {
                    href: "index.html".into(),
                    title: "Home".into(),
                }),
            ),
            hidden_page,
        ];

        let tree = build_site_nav_tree(&pages);
        assert_eq!(tree[0].children.len(), 1);
        assert_eq!(tree[0].children[0].title, "Visible");
    }

    #[test]
    fn test_nav_tree_nav_order() {
        let mut page_b = make_page(
            "b.html",
            "B",
            false,
            vec![],
            Some(NavLink {
                href: "index.html".into(),
                title: "Home".into(),
            }),
        );
        page_b.nav_order = Some(1);

        let mut page_a = make_page(
            "a.html",
            "A",
            false,
            vec![],
            Some(NavLink {
                href: "index.html".into(),
                title: "Home".into(),
            }),
        );
        page_a.nav_order = Some(2);

        let pages = vec![
            make_page(
                "index.html",
                "Home",
                true,
                vec![
                    NavLink {
                        href: "a.html".into(),
                        title: "A".into(),
                    },
                    NavLink {
                        href: "b.html".into(),
                        title: "B".into(),
                    },
                ],
                None,
            ),
            page_a,
            page_b,
        ];

        let tree = build_site_nav_tree(&pages);
        // B has nav_order 1, A has 2 — B should come first
        assert_eq!(tree[0].children[0].title, "B");
        assert_eq!(tree[0].children[1].title, "A");
    }

    #[test]
    fn test_nav_tree_nav_title() {
        let mut page_a = make_page(
            "a.html",
            "Full Title of A",
            false,
            vec![],
            Some(NavLink {
                href: "index.html".into(),
                title: "Home".into(),
            }),
        );
        page_a.nav_title = Some("Short A".to_string());

        let pages = vec![
            make_page(
                "index.html",
                "Home",
                true,
                vec![NavLink {
                    href: "a.html".into(),
                    title: "Full Title of A".into(),
                }],
                None,
            ),
            page_a,
        ];

        let tree = build_site_nav_tree(&pages);
        assert_eq!(tree[0].children[0].title, "Short A");
    }

    #[test]
    fn test_nav_for_page_marks_current_and_ancestors() {
        let pages = vec![
            make_page(
                "index.html",
                "Root",
                true,
                vec![NavLink {
                    href: "parent.html".into(),
                    title: "Parent".into(),
                }],
                None,
            ),
            make_page(
                "parent.html",
                "Parent",
                false,
                vec![NavLink {
                    href: "child.html".into(),
                    title: "Child".into(),
                }],
                Some(NavLink {
                    href: "index.html".into(),
                    title: "Root".into(),
                }),
            ),
            make_page(
                "child.html",
                "Child",
                false,
                vec![],
                Some(NavLink {
                    href: "parent.html".into(),
                    title: "Parent".into(),
                }),
            ),
        ];

        let tree = build_site_nav_tree(&pages);
        let nav = nav_for_page(&tree, "child.html", &pages);

        // Root should be ancestor
        assert!(nav.tree[0].is_ancestor_of_current);
        assert!(!nav.tree[0].is_current);

        // Parent should be ancestor
        assert!(nav.tree[0].children[0].is_ancestor_of_current);
        assert!(!nav.tree[0].children[0].is_current);

        // Child should be current
        assert!(nav.tree[0].children[0].children[0].is_current);
        assert!(!nav.tree[0].children[0].children[0].is_ancestor_of_current);

        // Breadcrumbs: Root → Parent → Child
        assert_eq!(nav.breadcrumbs.len(), 3);
        assert_eq!(nav.breadcrumbs[0].title, "Root");
        assert_eq!(nav.breadcrumbs[1].title, "Parent");
        assert_eq!(nav.breadcrumbs[2].title, "Child");
    }
}
