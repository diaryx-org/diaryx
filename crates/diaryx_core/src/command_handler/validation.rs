//! Validation and fix operation command handlers.

use std::path::PathBuf;

use crate::command::Response;
use crate::diaryx::Diaryx;
use crate::error::{DiaryxError, Result};
use crate::fs::AsyncFileSystem;

impl<FS: AsyncFileSystem + Clone> Diaryx<FS> {
    pub(crate) async fn cmd_validate_workspace(&self, path: Option<String>) -> Result<Response> {
        let root_path = path.ok_or_else(|| DiaryxError::InvalidPath {
            path: PathBuf::new(),
            message: "ValidateWorkspace requires a root index path".to_string(),
        })?;
        let resolved_root_path = self.resolve_fs_path(&root_path);
        let result = self
            .validate()
            .validate_workspace(&resolved_root_path, Some(2))
            .await?;
        Ok(Response::ValidationResult(result.with_metadata()))
    }

    pub(crate) async fn cmd_validate_file(&self, path: String) -> Result<Response> {
        let resolved_path = self.resolve_fs_path(&path);
        let result = self.validate().validate_file(&resolved_path).await?;
        Ok(Response::ValidationResult(result.with_metadata()))
    }

    pub(crate) async fn cmd_fix_broken_part_of(&self, path: String) -> Result<Response> {
        let resolved_path = self.resolve_fs_path(&path);
        let result = self
            .validate()
            .fixer()
            .fix_broken_part_of(&resolved_path)
            .await;

        if result.success {
            self.emit_workspace_sync().await;
        }

        Ok(Response::FixResult(result))
    }

    pub(crate) async fn cmd_fix_broken_contents_ref(
        &self,
        index_path: String,
        target: String,
    ) -> Result<Response> {
        let resolved_index_path = self.resolve_fs_path(&index_path);
        let result = self
            .validate()
            .fixer()
            .fix_broken_contents_ref(&resolved_index_path, &target)
            .await;

        if result.success {
            self.emit_workspace_sync().await;
        }

        Ok(Response::FixResult(result))
    }

    pub(crate) async fn cmd_fix_broken_attachment(
        &self,
        path: String,
        attachment: String,
    ) -> Result<Response> {
        let resolved_path = self.resolve_fs_path(&path);
        let result = self
            .validate()
            .fixer()
            .fix_broken_attachment(&resolved_path, &attachment)
            .await;

        if result.success {
            self.emit_workspace_sync().await;
        }

        Ok(Response::FixResult(result))
    }

    pub(crate) async fn cmd_fix_non_portable_path(
        &self,
        path: String,
        property: String,
        old_value: String,
        new_value: String,
    ) -> Result<Response> {
        let resolved_path = self.resolve_fs_path(&path);
        let result = self
            .validate()
            .fixer()
            .fix_non_portable_path(&resolved_path, &property, &old_value, &new_value)
            .await;

        if result.success {
            self.emit_workspace_sync().await;
        }

        Ok(Response::FixResult(result))
    }

    pub(crate) async fn cmd_fix_unlisted_file(
        &self,
        index_path: String,
        file_path: String,
    ) -> Result<Response> {
        let resolved_index_path = self.resolve_fs_path(&index_path);
        let resolved_file_path = self.resolve_fs_path(&file_path);
        let result = self
            .validate()
            .fixer()
            .fix_unlisted_file(&resolved_index_path, &resolved_file_path)
            .await;

        if result.success {
            self.emit_workspace_sync().await;
        }

        Ok(Response::FixResult(result))
    }

    pub(crate) async fn cmd_fix_orphan_binary_file(
        &self,
        index_path: String,
        file_path: String,
    ) -> Result<Response> {
        let resolved_index_path = self.resolve_fs_path(&index_path);
        let resolved_file_path = self.resolve_fs_path(&file_path);
        let result = self
            .validate()
            .fixer()
            .fix_orphan_binary_file(&resolved_index_path, &resolved_file_path)
            .await;

        if result.success {
            self.emit_workspace_sync().await;
        }

        Ok(Response::FixResult(result))
    }

    pub(crate) async fn cmd_fix_missing_part_of(
        &self,
        file_path: String,
        index_path: String,
    ) -> Result<Response> {
        let resolved_file_path = self.resolve_fs_path(&file_path);
        let resolved_index_path = self.resolve_fs_path(&index_path);
        let result = self
            .validate()
            .fixer()
            .fix_missing_part_of(&resolved_file_path, &resolved_index_path)
            .await;

        if result.success {
            self.emit_workspace_sync().await;
        }

        Ok(Response::FixResult(result))
    }

    pub(crate) async fn cmd_fix_all(
        &self,
        validation_result: crate::validate::ValidationResult,
    ) -> Result<Response> {
        let fixer = self.validate().fixer();
        let (error_fixes, warning_fixes) = fixer.fix_all(&validation_result).await;

        let total_fixed = error_fixes.iter().filter(|r| r.success).count()
            + warning_fixes.iter().filter(|r| r.success).count();
        let total_failed = error_fixes.iter().filter(|r| !r.success).count()
            + warning_fixes.iter().filter(|r| !r.success).count();

        if total_fixed > 0 {
            self.emit_workspace_sync().await;
        }

        Ok(Response::FixSummary(crate::command::FixSummary {
            error_fixes,
            warning_fixes,
            total_fixed,
            total_failed,
        }))
    }

    pub(crate) async fn cmd_fix_circular_reference(
        &self,
        file_path: String,
        part_of_value: String,
    ) -> Result<Response> {
        let resolved_file_path = self.resolve_fs_path(&file_path);
        let result = self
            .validate()
            .fixer()
            .fix_circular_reference(&resolved_file_path, &part_of_value)
            .await;

        if result.success {
            self.emit_workspace_sync().await;
        }

        Ok(Response::FixResult(result))
    }

    /// Generic "fix any warning" entry point.
    ///
    /// Delegates to `ValidationFixer::fix_warning`, which matches the variant
    /// and calls the appropriate per-variant fixer. Callers (CLI, web,
    /// Tauri) can use this instead of picking a variant-specific `Fix*`
    /// command, keeping the command surface stable as new warning variants
    /// are added. Returns a `FixResult::failure` if the variant is not
    /// auto-fixable.
    pub(crate) async fn cmd_fix_validation_warning(
        &self,
        warning: crate::validate::ValidationWarning,
    ) -> Result<Response> {
        let fixer = self.validate().fixer();
        let result = match fixer.fix_warning(&warning).await {
            Some(r) => r,
            None => crate::validate::FixResult::failure(format!(
                "Warning '{}' is not auto-fixable",
                warning.description()
            )),
        };

        if result.success {
            self.emit_workspace_sync().await;
        }

        Ok(Response::FixResult(result))
    }

    /// Generic "fix any error" entry point. See
    /// [`cmd_fix_validation_warning`].
    pub(crate) async fn cmd_fix_validation_error(
        &self,
        error: crate::validate::ValidationError,
    ) -> Result<Response> {
        let fixer = self.validate().fixer();
        let result = fixer.fix_error(&error).await;

        if result.success {
            self.emit_workspace_sync().await;
        }

        Ok(Response::FixResult(result))
    }
}
