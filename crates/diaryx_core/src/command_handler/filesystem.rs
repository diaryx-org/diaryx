//! Filesystem operation command handlers.

use crate::command::Response;
use crate::diaryx::Diaryx;
use crate::error::{DiaryxError, Result};
use crate::fs::AsyncFileSystem;

impl<FS: AsyncFileSystem + Clone> Diaryx<FS> {
    pub(crate) async fn cmd_file_exists(&self, path: String) -> Result<Response> {
        let resolved_path = self.resolve_fs_path(&path);
        let exists = self.fs().exists(&resolved_path).await;
        Ok(Response::Bool(exists))
    }

    pub(crate) async fn cmd_read_file(&self, path: String) -> Result<Response> {
        let content = self.entry().read_raw(&path).await?;
        Ok(Response::String(content))
    }

    pub(crate) async fn cmd_write_file(&self, path: String, content: String) -> Result<Response> {
        let resolved_path = self.resolve_fs_path(&path);
        self.fs()
            .write_file(&resolved_path, &content)
            .await
            .map_err(|e| DiaryxError::FileWrite {
                path: resolved_path.clone(),
                source: e,
            })?;
        Ok(Response::Ok)
    }

    pub(crate) async fn cmd_delete_file(&self, path: String) -> Result<Response> {
        let resolved_path = self.resolve_fs_path(&path);
        self.fs()
            .delete_file(&resolved_path)
            .await
            .map_err(|e| DiaryxError::FileWrite {
                path: resolved_path,
                source: e,
            })?;
        Ok(Response::Ok)
    }

    pub(crate) async fn cmd_clear_directory(&self, path: String) -> Result<Response> {
        let resolved_path = self.resolve_fs_path(&path);
        self.fs()
            .clear_dir(&resolved_path)
            .await
            .map_err(|e| DiaryxError::FileWrite {
                path: resolved_path,
                source: e,
            })?;
        Ok(Response::Ok)
    }

    pub(crate) async fn cmd_write_file_with_metadata(
        &self,
        path: String,
        metadata: serde_json::Value,
        body: String,
    ) -> Result<Response> {
        let resolved_path = self.resolve_fs_path(&path);
        crate::metadata_writer::write_file_with_metadata(
            self.fs(),
            &resolved_path,
            &metadata,
            &body,
        )
        .await?;
        Ok(Response::Ok)
    }

    pub(crate) async fn cmd_update_file_metadata(
        &self,
        path: String,
        metadata: serde_json::Value,
        body: Option<String>,
    ) -> Result<Response> {
        let resolved_path = self.resolve_fs_path(&path);
        crate::metadata_writer::update_file_metadata(
            self.fs(),
            &resolved_path,
            &metadata,
            body.as_deref(),
        )
        .await?;
        Ok(Response::Ok)
    }
}
