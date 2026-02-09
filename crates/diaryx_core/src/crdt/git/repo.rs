//! Git repository management — create and open repos for workspace history.

use std::path::Path;

use git2::Repository;

use crate::error::DiaryxError;

/// Whether to create a standard or bare repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoKind {
    /// Standard repo with working directory (for CLI workspaces).
    Standard,
    /// Bare repo without working directory (for server-side storage).
    Bare,
}

/// Initialize a new git repository at the given path.
///
/// Writes a `.gitignore` that excludes the `.diaryx/` directory (where the
/// SQLite CRDT database lives).
pub fn init_repo(path: &Path, kind: RepoKind) -> Result<Repository, DiaryxError> {
    let repo = match kind {
        RepoKind::Standard => {
            Repository::init(path).map_err(|e| DiaryxError::Git(e.to_string()))?
        }
        RepoKind::Bare => {
            Repository::init_bare(path).map_err(|e| DiaryxError::Git(e.to_string()))?
        }
    };

    // Write .gitignore for standard repos to exclude CRDT database.
    if kind == RepoKind::Standard {
        let gitignore_path = path.join(".gitignore");
        if !gitignore_path.exists() {
            std::fs::write(&gitignore_path, ".diaryx/\n")
                .map_err(|e| DiaryxError::Git(format!("Failed to write .gitignore: {}", e)))?;
        }
    }

    Ok(repo)
}

/// Open an existing git repository at the given path.
///
/// This discovers the repository by searching upwards from `path`.
pub fn open_repo(path: &Path) -> Result<Repository, DiaryxError> {
    Repository::open(path).map_err(|e| DiaryxError::Git(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_standard_repo() {
        let dir = tempfile::tempdir().unwrap();
        let repo = init_repo(dir.path(), RepoKind::Standard).unwrap();
        assert!(!repo.is_bare());
        assert!(dir.path().join(".git").exists());
        assert!(dir.path().join(".gitignore").exists());

        let gitignore = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert!(gitignore.contains(".diaryx/"));
    }

    #[test]
    fn test_init_bare_repo() {
        let dir = tempfile::tempdir().unwrap();
        let bare_path = dir.path().join("repo.git");
        let repo = init_repo(&bare_path, RepoKind::Bare).unwrap();
        assert!(repo.is_bare());
    }

    #[test]
    fn test_open_repo() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), RepoKind::Standard).unwrap();
        let repo = open_repo(dir.path()).unwrap();
        assert!(!repo.is_bare());
    }

    #[test]
    fn test_open_nonexistent_repo() {
        let dir = tempfile::tempdir().unwrap();
        let result = open_repo(dir.path());
        assert!(result.is_err());
    }
}
