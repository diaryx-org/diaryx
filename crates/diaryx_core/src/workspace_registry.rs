//! Multi-workspace registry types shared across all Diaryx frontends.
//!
//! Defines [`WorkspaceEntry`] and [`WorkspaceRegistry`] — the common data model
//! for tracking multiple workspaces. Platform-specific fields (e.g. OPFS storage
//! type, plugin metadata) live in the frontend layer; only the stable core fields
//! are defined here.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A single workspace known to the user.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct WorkspaceEntry {
    /// Stable identifier (`local-<uuid>`).
    pub id: String,
    /// Display name.
    pub name: String,
    /// Filesystem path. `Some` for native clients, `None` for browser-only (OPFS).
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub path: Option<PathBuf>,
}

/// An ordered collection of workspace entries with an optional default.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct WorkspaceRegistry {
    /// All known workspace entries.
    pub entries: Vec<WorkspaceEntry>,
    /// ID of the default workspace (must match an entry's `id`).
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub default_id: Option<String>,
}

impl WorkspaceRegistry {
    /// Look up an entry by its stable ID.
    pub fn find_by_id(&self, id: &str) -> Option<&WorkspaceEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Look up an entry by display name (case-sensitive).
    pub fn find_by_name(&self, name: &str) -> Option<&WorkspaceEntry> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// Look up an entry by filesystem path.
    pub fn find_by_path(&self, path: &Path) -> Option<&WorkspaceEntry> {
        self.entries
            .iter()
            .find(|e| e.path.as_deref() == Some(path))
    }

    /// Register a new workspace, returning a reference to the created entry.
    ///
    /// Generates a `local-<uuid>` ID. If a workspace with the same path already
    /// exists, returns a reference to the existing entry instead.
    #[cfg(feature = "uuid")]
    pub fn register(&mut self, name: String, path: Option<PathBuf>) -> &WorkspaceEntry {
        // Dedup by path if one is provided
        if let Some(ref p) = path
            && let Some(idx) = self
                .entries
                .iter()
                .position(|e| e.path.as_deref() == Some(p))
        {
            return &self.entries[idx];
        }

        let entry = WorkspaceEntry {
            id: format!("local-{}", uuid::Uuid::new_v4()),
            name,
            path,
        };
        self.entries.push(entry);
        self.entries.last().unwrap()
    }

    /// Remove a workspace by ID. Returns the removed entry if found.
    pub fn unregister(&mut self, id: &str) -> Option<WorkspaceEntry> {
        if let Some(idx) = self.entries.iter().position(|e| e.id == id) {
            let entry = self.entries.remove(idx);
            // Clear default if it pointed to the removed entry
            if self.default_id.as_deref() == Some(id) {
                self.default_id = None;
            }
            Some(entry)
        } else {
            None
        }
    }

    /// Rename a workspace. Returns `true` if the entry was found.
    pub fn rename(&mut self, id: &str, new_name: String) -> bool {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
            entry.name = new_name;
            true
        } else {
            false
        }
    }

    /// Set the default workspace. Returns `true` if the ID exists in the registry.
    pub fn set_default(&mut self, id: &str) -> bool {
        if self.entries.iter().any(|e| e.id == id) {
            self.default_id = Some(id.to_string());
            true
        } else {
            false
        }
    }

    /// Get the default workspace entry, if one is set and still exists.
    pub fn default_entry(&self) -> Option<&WorkspaceEntry> {
        self.default_id
            .as_deref()
            .and_then(|id| self.find_by_id(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_generates_unique_ids() {
        let mut reg = WorkspaceRegistry::default();
        let id1 = reg.register("a".into(), None).id.clone();
        let id2 = reg.register("b".into(), None).id.clone();
        assert_ne!(id1, id2);
        assert!(id1.starts_with("local-"));
        assert!(id2.starts_with("local-"));
    }

    #[test]
    fn register_deduplicates_by_path() {
        let mut reg = WorkspaceRegistry::default();
        let path = PathBuf::from("/home/user/journal");
        let id1 = reg
            .register("journal".into(), Some(path.clone()))
            .id
            .clone();
        let id2 = reg.register("journal-dup".into(), Some(path)).id.clone();
        assert_eq!(id1, id2);
        assert_eq!(reg.entries.len(), 1);
    }

    #[test]
    fn find_by_id_name_path() {
        let mut reg = WorkspaceRegistry::default();
        let path = PathBuf::from("/ws");
        let id = reg.register("my-ws".into(), Some(path.clone())).id.clone();

        assert!(reg.find_by_id(&id).is_some());
        assert!(reg.find_by_name("my-ws").is_some());
        assert!(reg.find_by_path(&path).is_some());
        assert!(reg.find_by_name("nonexistent").is_none());
    }

    #[test]
    fn unregister_removes_and_clears_default() {
        let mut reg = WorkspaceRegistry::default();
        let id = reg.register("ws".into(), None).id.clone();
        reg.set_default(&id);
        assert!(reg.default_entry().is_some());

        let removed = reg.unregister(&id);
        assert!(removed.is_some());
        assert!(reg.default_entry().is_none());
        assert_eq!(reg.entries.len(), 0);
    }

    #[test]
    fn unregister_nonexistent_returns_none() {
        let mut reg = WorkspaceRegistry::default();
        assert!(reg.unregister("nope").is_none());
    }

    #[test]
    fn rename_entry() {
        let mut reg = WorkspaceRegistry::default();
        let id = reg.register("old".into(), None).id.clone();
        assert!(reg.rename(&id, "new".into()));
        assert_eq!(reg.find_by_id(&id).unwrap().name, "new");
        assert!(!reg.rename("bad-id", "x".into()));
    }

    #[test]
    fn set_default_and_default_entry() {
        let mut reg = WorkspaceRegistry::default();
        let id = reg.register("ws".into(), None).id.clone();

        assert!(reg.default_entry().is_none());
        assert!(reg.set_default(&id));
        assert_eq!(reg.default_entry().unwrap().id, id);
        assert!(!reg.set_default("nonexistent"));
    }
}
