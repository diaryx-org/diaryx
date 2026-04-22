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

        Ok(Response::FixSummary(crate::command::FixSummary {
            error_fixes,
            warning_fixes,
            total_fixed,
            total_failed,
        }))
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

        Ok(Response::FixResult(result))
    }
}
