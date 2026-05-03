//! Frontmatter operation command handlers.

use std::path::Path;

use crate::yaml;

use crate::command::Response;
use crate::diaryx::Diaryx;
use crate::error::DiaryxError;
use crate::error::Result;
use crate::fs::AsyncFileSystem;

impl<FS: AsyncFileSystem + Clone> Diaryx<FS> {
    fn extract_markdown_link_destinations(content: &str) -> Vec<String> {
        let bytes = content.as_bytes();
        let mut links = Vec::new();
        let mut i = 0;

        while i < bytes.len() {
            if bytes[i] != b'[' {
                i += 1;
                continue;
            }
            if i > 0 && bytes[i - 1] == b'!' {
                i += 1;
                continue;
            }

            let mut label_end = i + 1;
            while label_end < bytes.len() {
                match bytes[label_end] {
                    b'\\' => label_end += 2,
                    b']' => break,
                    _ => label_end += 1,
                }
            }

            if label_end >= bytes.len()
                || bytes[label_end] != b']'
                || label_end + 1 >= bytes.len()
                || bytes[label_end + 1] != b'('
            {
                i += 1;
                continue;
            }

            let href_start = label_end + 2;
            let mut cursor = href_start;
            let mut depth = 1usize;
            while cursor < bytes.len() {
                match bytes[cursor] {
                    b'\\' => cursor += 2,
                    b'(' => {
                        depth += 1;
                        cursor += 1;
                    }
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            let href = content[href_start..cursor].trim();
                            let href = href
                                .strip_prefix('<')
                                .and_then(|s| s.strip_suffix('>'))
                                .unwrap_or(href);
                            if !href.is_empty() {
                                links.push(href.to_string());
                            }
                            cursor += 1;
                            break;
                        }
                        cursor += 1;
                    }
                    _ => cursor += 1,
                }
            }

            i = cursor.max(i + 1);
        }

        links
    }

    fn is_local_body_link(href: &str) -> bool {
        let lowered = href.trim().to_ascii_lowercase();
        !(lowered.contains("://")
            || lowered.starts_with("mailto:")
            || lowered.starts_with("tel:")
            || lowered.starts_with('#')
            || lowered.starts_with("javascript:"))
    }

    async fn content_uses_target(
        &self,
        source_path: &str,
        target_canonical: &str,
        content: Option<&str>,
    ) -> bool {
        let source_canonical = self.get_canonical_path(source_path);
        let body = match content {
            Some(content) => content.to_string(),
            None => self
                .entry()
                .get_content(source_path)
                .await
                .unwrap_or_default(),
        };

        Self::extract_markdown_link_destinations(&body)
            .into_iter()
            .filter(|href| Self::is_local_body_link(href))
            .any(|href| {
                self.resolve_frontmatter_link_target(&href, &source_canonical) == target_canonical
            })
    }

    async fn upsert_frontmatter_link_array_item(
        &self,
        file_path: &str,
        key: &str,
        target_canonical: &str,
    ) -> Result<bool> {
        let file_canonical = self.get_canonical_path(file_path);
        let existing = self
            .entry()
            .get_frontmatter_property(file_path, key)
            .await?;
        let mut items = match existing {
            Some(yaml::Value::Sequence(items)) => items,
            Some(_) => Vec::new(),
            None => Vec::new(),
        };

        let already_present = items.iter().any(|item| {
            item.as_str().is_some_and(|s| {
                self.resolve_frontmatter_link_target(s, &file_canonical) == target_canonical
            })
        });
        if already_present {
            return Ok(false);
        }

        let formatted = self.format_link_for_file(target_canonical, &file_canonical);
        items.push(yaml::Value::String(formatted));
        self.entry()
            .set_frontmatter_property(file_path, key, yaml::Value::Sequence(items))
            .await?;
        Ok(true)
    }

    async fn remove_frontmatter_link_array_item(
        &self,
        file_path: &str,
        key: &str,
        target_canonical: &str,
    ) -> Result<bool> {
        let file_canonical = self.get_canonical_path(file_path);
        let existing = self
            .entry()
            .get_frontmatter_property(file_path, key)
            .await?;
        let Some(yaml::Value::Sequence(items)) = existing else {
            return Ok(false);
        };
        let original_len = items.len();

        let filtered: Vec<yaml::Value> = items
            .into_iter()
            .filter(|item| {
                !item.as_str().is_some_and(|s| {
                    self.resolve_frontmatter_link_target(s, &file_canonical) == target_canonical
                })
            })
            .collect();

        let changed = filtered.len() != original_len;
        if !changed {
            return Ok(false);
        }

        if filtered.is_empty() {
            self.entry()
                .remove_frontmatter_property(file_path, key)
                .await?;
        } else {
            self.entry()
                .set_frontmatter_property(file_path, key, yaml::Value::Sequence(filtered))
                .await?;
        }
        Ok(true)
    }

    async fn ensure_self_link_property(&self, file_path: &str) -> Result<bool> {
        let canonical_path = self.get_canonical_path(file_path);
        match self
            .entry()
            .get_frontmatter_property(file_path, "link")
            .await?
        {
            Some(yaml::Value::String(existing))
                if self.resolve_frontmatter_link_target(&existing, &canonical_path)
                    == canonical_path =>
            {
                Ok(false)
            }
            Some(_) => Ok(false),
            None => {
                let formatted = self.format_link_for_file(&canonical_path, &canonical_path);
                self.entry()
                    .set_frontmatter_property(file_path, "link", yaml::Value::String(formatted))
                    .await?;
                Ok(true)
            }
        }
    }

    pub(crate) async fn cmd_get_frontmatter(&self, path: String) -> Result<Response> {
        let fm = self.entry().get_frontmatter(&path).await?;
        Ok(Response::Frontmatter(fm))
    }

    pub(crate) async fn cmd_set_frontmatter_property(
        &self,
        path: String,
        key: String,
        value: yaml::Value,
        root_index_path: Option<String>,
    ) -> Result<Response> {
        // Handle link/part_of/contents/attachments specially - normalize and
        // format links according to workspace settings.
        {
            let canonical_path = self.get_canonical_path(&path);

            if key == "link" {
                if let yaml::Value::String(ref s) = value {
                    let canonical_target = self.resolve_frontmatter_link_target(s, &canonical_path);
                    let formatted = self.format_link_for_file(&canonical_target, &canonical_path);
                    let yaml_value = yaml::Value::String(formatted);
                    self.entry()
                        .set_frontmatter_property(&path, &key, yaml_value)
                        .await?;
                    return Ok(Response::Ok);
                }
            } else if key == "attachment" {
                if let yaml::Value::String(ref s) = value {
                    let canonical_target = self.resolve_attachment_link_target_with_hint(
                        s,
                        &canonical_path,
                        Some(crate::link_parser::LinkFormat::PlainCanonical),
                    );
                    let formatted =
                        self.format_attachment_link_for_file(&canonical_target, &canonical_path);
                    let yaml_value = yaml::Value::String(formatted);
                    self.entry()
                        .set_frontmatter_property(&path, &key, yaml_value)
                        .await?;
                    return Ok(Response::Ok);
                }
            } else if key == "part_of" {
                if let yaml::Value::String(ref s) = value {
                    let canonical_target = self.resolve_frontmatter_link_target(s, &canonical_path);
                    let formatted = self.format_link_for_file(&canonical_target, &canonical_path);
                    let yaml_value = yaml::Value::String(formatted);
                    self.entry()
                        .set_frontmatter_property(&path, &key, yaml_value)
                        .await?;
                    return Ok(Response::Ok);
                }
            } else if key == "contents"
                || key == "links"
                || key == "link_of"
                || key == "attachment_of"
            {
                if let yaml::Value::Sequence(ref arr) = value {
                    let mut formatted_links: Vec<yaml::Value> = Vec::new();

                    for item in arr {
                        if let yaml::Value::String(s) = item {
                            let canonical_target = self.resolve_attachment_link_target_with_hint(
                                s,
                                &canonical_path,
                                Some(self.link_format()),
                            );
                            let formatted =
                                self.format_link_for_file(&canonical_target, &canonical_path);
                            formatted_links.push(yaml::Value::String(formatted));
                        }
                    }

                    let yaml_value = yaml::Value::Sequence(formatted_links);
                    self.entry()
                        .set_frontmatter_property(&path, &key, yaml_value)
                        .await?;
                    return Ok(Response::Ok);
                }
            } else if key == "attachments" {
                // Attachments now point to attachment notes, not binary assets.
                if let yaml::Value::Sequence(ref arr) = value {
                    let mut formatted_links: Vec<yaml::Value> = Vec::new();

                    for item in arr {
                        if let yaml::Value::String(s) = item {
                            let canonical_target =
                                self.resolve_frontmatter_link_target(s, &canonical_path);
                            let formatted =
                                self.format_link_for_file(&canonical_target, &canonical_path);
                            formatted_links.push(yaml::Value::String(formatted));
                        }
                    }

                    let yaml_value = yaml::Value::Sequence(formatted_links);
                    self.entry()
                        .set_frontmatter_property(&path, &key, yaml_value)
                        .await?;
                    return Ok(Response::Ok);
                } else if let yaml::Value::String(ref s) = value {
                    let canonical_target = self.resolve_attachment_link_target_with_hint(
                        s,
                        &canonical_path,
                        Some(self.link_format()),
                    );
                    let formatted = self.format_link_for_file(&canonical_target, &canonical_path);
                    let yaml_value = yaml::Value::String(formatted);
                    self.entry()
                        .set_frontmatter_property(&path, &key, yaml_value)
                        .await?;
                    return Ok(Response::Ok);
                }
            }
        }

        // Auto-rename on title change + sync heading
        if key == "title"
            && let Some(ref rip) = root_index_path
            && let yaml::Value::String(ref new_title) = value
            && !new_title.trim().is_empty()
        {
            use crate::entry::apply_filename_style;

            let ws_config = self
                .workspace()
                .inner()
                .get_workspace_config(&self.resolve_fs_path(rip))
                .await
                .unwrap_or_default();

            let mut effective_path = path.clone();

            // Write the title FIRST so that rename_entry's resolve_title
            // reads the new title when formatting links in parent contents
            // and children's part_of references.
            self.entry()
                .set_frontmatter_property(&path, &key, value.clone())
                .await?;

            // Always auto-rename file to match title
            {
                let new_stem = apply_filename_style(new_title, &ws_config.filename_style);
                let new_filename = format!("{}.md", new_stem);

                let entry_path = self.resolve_fs_path(&path);
                let ws = self.workspace().inner();
                let is_index = ws.is_index_file(&entry_path).await;
                let is_root = ws.is_root_index(&entry_path).await;

                // Compare current name:
                // - Non-root index: dir name (index lives in dirname/dirname.md)
                // - Root index or leaf: file stem
                let current_comparable = if is_index && !is_root {
                    entry_path
                        .parent()
                        .and_then(|p| p.file_name())
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_string()
                } else {
                    entry_path
                        .file_stem()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_string()
                };

                if current_comparable != new_stem {
                    let new_path = ws.rename_entry(&entry_path, &new_filename).await?;
                    effective_path = new_path.to_string_lossy().to_string();
                }
            }

            // Always sync title to H1 heading
            self.sync_heading_to_title(&effective_path, new_title)
                .await?;

            // Return new path if rename happened, Ok otherwise
            if effective_path != path {
                return Ok(Response::String(effective_path));
            } else {
                return Ok(Response::Ok);
            }
        }

        // Default: just set the property as-is (non-title keys, or title without root_index_path)
        self.entry()
            .set_frontmatter_property(&path, &key, value.clone())
            .await?;

        Ok(Response::Ok)
    }

    pub(crate) async fn cmd_remove_frontmatter_property(
        &self,
        path: String,
        key: String,
    ) -> Result<Response> {
        self.entry()
            .remove_frontmatter_property(&path, &key)
            .await?;

        Ok(Response::Ok)
    }

    pub(crate) async fn cmd_add_link(
        &self,
        source_path: String,
        target_path: String,
        _content: Option<String>,
    ) -> Result<Response> {
        let source_fs_path = self.resolve_fs_path(&source_path);
        let target_fs_path = self.resolve_fs_path(&target_path);
        if !self.fs().exists(&source_fs_path).await {
            return Err(DiaryxError::Validation(format!(
                "Source entry not found: {}",
                Path::new(&source_path).display()
            )));
        }
        if !self.fs().exists(&target_fs_path).await {
            return Err(DiaryxError::Validation(format!(
                "Target entry not found: {}",
                Path::new(&target_path).display()
            )));
        }

        let source_canonical = self.get_canonical_path(&source_path);
        let target_canonical = self.get_canonical_path(&target_path);

        let mut changed = false;
        changed |= self
            .upsert_frontmatter_link_array_item(&source_path, "links", &target_canonical)
            .await?;
        changed |= self
            .upsert_frontmatter_link_array_item(&target_path, "link_of", &source_canonical)
            .await?;
        changed |= self.ensure_self_link_property(&target_path).await?;
        let _ = changed;

        Ok(Response::Ok)
    }

    pub(crate) async fn cmd_remove_link(
        &self,
        source_path: String,
        target_path: String,
        content: Option<String>,
    ) -> Result<Response> {
        let source_fs_path = self.resolve_fs_path(&source_path);
        if !self.fs().exists(&source_fs_path).await {
            return Err(DiaryxError::Validation(format!(
                "Source entry not found: {}",
                Path::new(&source_path).display()
            )));
        }

        let target_canonical = self.get_canonical_path(&target_path);
        if self
            .content_uses_target(&source_path, &target_canonical, content.as_deref())
            .await
        {
            return Ok(Response::Ok);
        }

        let source_canonical = self.get_canonical_path(&source_path);
        let mut changed = false;
        changed |= self
            .remove_frontmatter_link_array_item(&source_path, "links", &target_canonical)
            .await?;

        let target_fs_path = self.resolve_fs_path(&target_path);
        if self.fs().exists(&target_fs_path).await {
            changed |= self
                .remove_frontmatter_link_array_item(&target_path, "link_of", &source_canonical)
                .await?;
        }
        let _ = changed;

        Ok(Response::Ok)
    }

    pub(crate) async fn cmd_reorder_frontmatter_keys(
        &self,
        path: String,
        keys: Vec<String>,
    ) -> Result<Response> {
        self.entry().reorder_frontmatter_keys(&path, &keys).await?;
        Ok(Response::Ok)
    }

    pub(crate) async fn cmd_move_frontmatter_section_to_file(
        &self,
        source_path: String,
        section_key: String,
        target_path: String,
        create_if_missing: bool,
    ) -> Result<Response> {
        self.entry()
            .move_frontmatter_section_to_file(
                &source_path,
                &section_key,
                &target_path,
                create_if_missing,
            )
            .await?;
        Ok(Response::Ok)
    }
}
