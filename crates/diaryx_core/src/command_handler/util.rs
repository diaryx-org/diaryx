//! Utility command handlers (all sync -- no async state machine overhead).

use std::path::Path;

use crate::command::Response;
use crate::diaryx::Diaryx;
use crate::error::{DiaryxError, Result};
use crate::fs::AsyncFileSystem;
use crate::link_parser;

impl<FS: AsyncFileSystem + Clone> Diaryx<FS> {
    pub(crate) fn cmd_get_storage_usage(&self) -> Result<Response> {
        Ok(Response::StorageInfo(crate::command::StorageInfo {
            used: 0,
            limit: None,
            attachment_limit: None,
        }))
    }

    pub(crate) fn cmd_link_parser(
        &self,
        operation: crate::command::LinkParserOperation,
    ) -> Result<Response> {
        let result = match operation {
            crate::command::LinkParserOperation::Parse { link } => {
                let parsed = link_parser::parse_link(&link);
                let path_type = match parsed.path_type {
                    link_parser::PathType::WorkspaceRoot => {
                        crate::command::LinkPathType::WorkspaceRoot
                    }
                    link_parser::PathType::Relative => crate::command::LinkPathType::Relative,
                    link_parser::PathType::Ambiguous => crate::command::LinkPathType::Ambiguous,
                };
                crate::command::LinkParserResult::Parsed(crate::command::ParsedLinkResult {
                    title: parsed.title,
                    path: parsed.path,
                    path_type,
                })
            }
            crate::command::LinkParserOperation::ToCanonical {
                link,
                current_file_path,
                link_format_hint,
            } => {
                let parsed = link_parser::parse_link(&link);
                let canonical = link_parser::to_canonical_with_link_format(
                    &parsed,
                    Path::new(&current_file_path),
                    link_format_hint,
                );
                crate::command::LinkParserResult::String(canonical)
            }
            crate::command::LinkParserOperation::Format {
                canonical_path,
                title,
                format,
                from_canonical_path,
            } => crate::command::LinkParserResult::String(link_parser::format_link_with_format(
                &canonical_path,
                &title,
                format,
                &from_canonical_path,
            )),
            crate::command::LinkParserOperation::Convert {
                link,
                target_format,
                current_file_path,
                source_format_hint,
            } => crate::command::LinkParserResult::String(link_parser::convert_link_with_hint(
                &link,
                target_format,
                &current_file_path,
                None,
                source_format_hint,
            )),
        };

        Ok(Response::LinkParserResult(result))
    }

    pub(crate) fn cmd_validate_workspace_name(
        &self,
        name: String,
        existing_local_names: Vec<String>,
        existing_server_names: Option<Vec<String>>,
    ) -> Result<Response> {
        use crate::utils::naming;
        naming::validate_workspace_name(
            &name,
            &existing_local_names,
            existing_server_names.as_deref(),
        )
        .map(Response::String)
        .map_err(DiaryxError::Validation)
    }

    pub(crate) fn cmd_validate_publishing_slug(&self, slug: String) -> Result<Response> {
        use crate::utils::naming;
        naming::validate_publishing_slug(&slug)
            .map(|()| Response::Ok)
            .map_err(DiaryxError::Validation)
    }

    pub(crate) fn cmd_normalize_server_url(&self, url: String) -> Result<Response> {
        use crate::utils::naming;
        Ok(Response::String(naming::normalize_server_url(&url)))
    }
}
