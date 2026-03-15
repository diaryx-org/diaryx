//! Workspace configuration command handlers.

use std::path::PathBuf;

use crate::command::Response;
use crate::diaryx::Diaryx;
use crate::error::{DiaryxError, Result};
use crate::fs::AsyncFileSystem;
use crate::link_parser;

impl<FS: AsyncFileSystem + Clone> Diaryx<FS> {
    pub(crate) async fn cmd_get_link_format(&self, root_index_path: String) -> Result<Response> {
        let ws = self.workspace().inner();
        let resolved_root_index_path = self.resolve_fs_path(&root_index_path);
        let format = ws.get_link_format(&resolved_root_index_path).await?;
        Ok(Response::LinkFormat(format))
    }

    pub(crate) async fn cmd_set_link_format(
        &self,
        root_index_path: String,
        format: String,
    ) -> Result<Response> {
        let link_format = match format.as_str() {
            "markdown_root" => link_parser::LinkFormat::MarkdownRoot,
            "markdown_relative" => link_parser::LinkFormat::MarkdownRelative,
            "plain_relative" => link_parser::LinkFormat::PlainRelative,
            "plain_canonical" => link_parser::LinkFormat::PlainCanonical,
            _ => {
                return Err(DiaryxError::InvalidPath {
                    path: PathBuf::from(&format),
                    message: format!(
                        "Invalid link format '{}'. Must be one of: markdown_root, markdown_relative, plain_relative, plain_canonical",
                        format
                    ),
                });
            }
        };

        let ws = self.workspace().inner();
        let resolved_root_index_path = self.resolve_fs_path(&root_index_path);
        ws.set_link_format(&resolved_root_index_path, link_format)
            .await?;
        Ok(Response::Ok)
    }

    pub(crate) async fn cmd_get_workspace_config(
        &self,
        root_index_path: String,
    ) -> Result<Response> {
        let ws = self.workspace().inner();
        let resolved_root_index_path = self.resolve_fs_path(&root_index_path);
        let config = ws.get_workspace_config(&resolved_root_index_path).await?;
        Ok(Response::WorkspaceConfig(config))
    }

    pub(crate) async fn cmd_generate_filename(
        &self,
        title: String,
        root_index_path: Option<String>,
    ) -> Result<Response> {
        use crate::entry::apply_filename_style;
        use crate::workspace::FilenameStyle;

        let style = if let Some(ref root_path) = root_index_path {
            let ws = self.workspace().inner();
            let resolved_root_path = self.resolve_fs_path(root_path);
            let config = ws.get_workspace_config(&resolved_root_path).await?;
            config.filename_style
        } else {
            FilenameStyle::default()
        };
        let stem = apply_filename_style(&title, &style);
        Ok(Response::String(format!("{}.md", stem)))
    }

    pub(crate) async fn cmd_set_workspace_config(
        &self,
        root_index_path: String,
        field: String,
        value: String,
    ) -> Result<Response> {
        let ws = self.workspace().inner();
        let resolved_root_index_path = self.resolve_fs_path(&root_index_path);
        ws.set_workspace_config_field(&resolved_root_index_path, &field, &value)
            .await?;
        Ok(Response::Ok)
    }

    pub(crate) async fn cmd_convert_links(
        &self,
        root_index_path: String,
        format: String,
        path: Option<String>,
        dry_run: bool,
    ) -> Result<Response> {
        let target_format = match format.as_str() {
            "markdown_root" => link_parser::LinkFormat::MarkdownRoot,
            "markdown_relative" => link_parser::LinkFormat::MarkdownRelative,
            "plain_relative" => link_parser::LinkFormat::PlainRelative,
            "plain_canonical" => link_parser::LinkFormat::PlainCanonical,
            _ => {
                return Err(DiaryxError::InvalidPath {
                    path: PathBuf::from(&format),
                    message: format!(
                        "Invalid link format '{}'. Must be one of: markdown_root, markdown_relative, plain_relative, plain_canonical",
                        format
                    ),
                });
            }
        };

        let resolved_root_index_path = self.resolve_fs_path(&root_index_path);
        let resolved_specific_path = path
            .as_deref()
            .map(|p| self.resolve_fs_path(p).to_string_lossy().to_string());
        let result = self
            .convert_workspace_links(
                &resolved_root_index_path,
                target_format,
                resolved_specific_path.as_deref(),
                dry_run,
            )
            .await?;

        Ok(Response::ConvertLinksResult(result))
    }
}
