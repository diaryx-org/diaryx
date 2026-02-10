//! Per-file document body CRDT.
//!
//! This module provides `BodyDoc`, a Y.Doc for collaborative editing of
//! individual file contents. Each file in the workspace can have its own
//! BodyDoc for real-time sync of markdown content.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use yrs::{
    Doc, GetString, Map, Observable, ReadTxn, Text, Transact, Update, updates::decoder::Decode,
    updates::encoder::Encode,
};

use super::storage::{CrdtStorage, StorageResult};
use super::types::UpdateOrigin;
use crate::error::DiaryxError;
use crate::fs::FileSystemEvent;

/// Name of the Y.Text holding the document body content.
const BODY_TEXT_NAME: &str = "body";

/// Name of the Y.Map holding frontmatter properties.
const FRONTMATTER_MAP_NAME: &str = "frontmatter";

/// Callback type for sync messages. Takes (doc_name, update_bytes).
pub type SyncCallback = Arc<dyn Fn(&str, &[u8]) + Send + Sync>;

/// A CRDT document for a single file's body content.
///
/// Each file in the workspace can have its own BodyDoc for collaborative
/// editing. The document contains:
/// - A Y.Text for the markdown body content
/// - A Y.Map for frontmatter properties (optional structured access)
///
/// # Example
///
/// ```ignore
/// use diaryx_core::crdt::{BodyDoc, MemoryStorage};
/// use std::sync::Arc;
///
/// let storage = Arc::new(MemoryStorage::new());
/// let doc = BodyDoc::new(storage, "workspace/notes/hello.md".to_string());
///
/// // Set content
/// doc.set_body("# Hello World\n\nThis is my note.");
///
/// // Get content
/// let body = doc.get_body();
/// assert!(body.starts_with("# Hello"));
/// ```
pub struct BodyDoc {
    doc: Doc,
    body_text: yrs::TextRef,
    frontmatter_map: yrs::MapRef,
    storage: Arc<dyn CrdtStorage>,
    /// The document name (file path). Uses RwLock for interior mutability
    /// to allow renaming via `set_doc_name()` without mutable access.
    doc_name: Arc<RwLock<String>>,
    /// Optional callback for emitting filesystem events on remote/sync updates.
    event_callback: Option<Arc<dyn Fn(&FileSystemEvent) + Send + Sync>>,
    /// Flag set during apply_update to prevent observer from firing for remote updates.
    /// This prevents echoing back updates we receive from the server.
    applying_remote: Arc<AtomicBool>,
    /// Callback to emit sync messages when local changes occur.
    sync_callback: RwLock<Option<SyncCallback>>,
    /// Stored observer subscription to keep it alive.
    /// With yrs "sync" feature enabled, Subscription is Send+Sync.
    _update_subscription: RwLock<Option<yrs::Subscription>>,
}

impl BodyDoc {
    /// Create a new empty body document.
    ///
    /// The document name should be the file path (e.g., "workspace/notes/hello.md").
    pub fn new(storage: Arc<dyn CrdtStorage>, doc_name: String) -> Self {
        let doc = Doc::new();
        let body_text = doc.get_or_insert_text(BODY_TEXT_NAME);
        let frontmatter_map = doc.get_or_insert_map(FRONTMATTER_MAP_NAME);

        Self {
            doc,
            body_text,
            frontmatter_map,
            storage,
            doc_name: Arc::new(RwLock::new(doc_name)),
            event_callback: None,
            applying_remote: Arc::new(AtomicBool::new(false)),
            sync_callback: RwLock::new(None),
            _update_subscription: RwLock::new(None),
        }
    }

    /// Load a body document from storage, or create a new one if it doesn't exist.
    ///
    /// This loads both the base snapshot (if any) and all incremental updates
    /// to reconstruct the current state. This is critical for WASM where updates
    /// are stored but snapshots may not be saved.
    pub fn load(storage: Arc<dyn CrdtStorage>, doc_name: String) -> StorageResult<Self> {
        let doc = Doc::new();
        let body_text = doc.get_or_insert_text(BODY_TEXT_NAME);
        let frontmatter_map = doc.get_or_insert_map(FRONTMATTER_MAP_NAME);

        {
            let mut txn = doc.transact_mut();

            // Try to load base snapshot from storage
            if let Some(state) = storage.load_doc(&doc_name)?
                && let Ok(update) = Update::decode_v1(&state)
                && let Err(e) = txn.apply_update(update)
            {
                log::warn!(
                    "Failed to apply stored snapshot for body doc {}: {}",
                    doc_name,
                    e
                );
            }

            // Apply all incremental updates from storage
            // This is critical for WASM where updates are stored but snapshots may not be saved
            let updates = storage.get_all_updates(&doc_name)?;
            for crdt_update in updates {
                if let Ok(update) = Update::decode_v1(&crdt_update.data)
                    && let Err(e) = txn.apply_update(update)
                {
                    log::warn!(
                        "Failed to apply stored update {} for body doc {}: {}",
                        crdt_update.update_id,
                        doc_name,
                        e
                    );
                }
            }
        }

        Ok(Self {
            doc,
            body_text,
            frontmatter_map,
            storage,
            doc_name: Arc::new(RwLock::new(doc_name)),
            event_callback: None,
            applying_remote: Arc::new(AtomicBool::new(false)),
            sync_callback: RwLock::new(None),
            _update_subscription: RwLock::new(None),
        })
    }

    /// Set the event callback for emitting filesystem events on remote/sync updates.
    ///
    /// When set, this callback will be invoked with `ContentsChanged` events whenever
    /// `apply_update()` is called with a non-Local origin.
    pub fn set_event_callback(&mut self, callback: Arc<dyn Fn(&FileSystemEvent) + Send + Sync>) {
        self.event_callback = Some(callback);
    }

    /// Set the sync callback for emitting sync messages when local changes occur.
    ///
    /// This uses the Yrs observer pattern: when any mutation occurs in the document
    /// (via `set_body()`, `insert_at()`, etc.), the observer automatically receives
    /// the exact update bytes to send. This is more reliable than manual delta encoding
    /// because Yrs handles state vector tracking internally.
    ///
    /// The callback receives (doc_name, update_bytes) and should send the update
    /// to the sync server.
    ///
    /// **Important**: The observer will NOT fire for remote updates (those applied via
    /// `apply_update()` with non-Local origin) because the `applying_remote` flag
    /// prevents echoing.
    pub fn set_sync_callback(&self, callback: SyncCallback) {
        let doc_name = self.doc_name.read().unwrap().clone();
        if self._update_subscription.read().unwrap().is_some() {
            log::trace!(
                "[BodyDoc] set_sync_callback: observer already registered for '{}', skipping",
                doc_name
            );
            return;
        }
        log::trace!(
            "[BodyDoc] set_sync_callback: registering observer for '{}'",
            doc_name
        );

        // Store the callback
        {
            let mut cb = self.sync_callback.write().unwrap();
            *cb = Some(callback.clone());
        }

        // Set up the update observer
        let applying_remote = Arc::clone(&self.applying_remote);
        let doc_name_ref = Arc::clone(&self.doc_name);

        let subscription = self
            .doc
            .observe_update_v1(move |_, event| {
                let current_doc_name = doc_name_ref.read().unwrap().clone();

                // Skip if this is a remote update (we don't want to echo it back)
                if applying_remote.load(Ordering::SeqCst) {
                    log::trace!(
                        "[BodyDoc] Observer skipping remote update for '{}'",
                        current_doc_name
                    );
                    return;
                }

                // Emit sync message with the update bytes
                log::trace!(
                    "[BodyDoc] Observer fired for '{}', update_len={}",
                    current_doc_name,
                    event.update.len()
                );
                callback(&current_doc_name, &event.update);
            })
            .expect("Failed to observe document updates");

        // Store subscription to keep it alive
        let mut sub = self._update_subscription.write().unwrap();
        *sub = Some(subscription);
        log::trace!(
            "[BodyDoc] set_sync_callback: observer registered for '{}'",
            doc_name
        );
    }

    /// Emit a filesystem event to the registered callback, if any.
    #[allow(dead_code)]
    fn emit_event(&self, event: FileSystemEvent) {
        if let Some(ref cb) = self.event_callback {
            cb(&event);
        }
    }

    /// Get the document name (file path).
    pub fn doc_name(&self) -> String {
        self.doc_name.read().unwrap().clone()
    }

    /// Set the document name (file path).
    ///
    /// This is used when renaming a file to update the internal doc_name
    /// without re-creating the document. Uses interior mutability to allow
    /// renaming through an Arc<BodyDoc>.
    pub fn set_doc_name(&self, new_name: String) {
        let mut name = self.doc_name.write().unwrap();
        *name = new_name;
    }

    // ==================== Body Content Operations ====================

    /// Get the full body content as a string.
    pub fn get_body(&self) -> String {
        let txn = self.doc.transact();
        self.body_text.get_string(&txn)
    }

    /// Set the body content, using minimal diff operations.
    ///
    /// Instead of delete-all + insert-all (which breaks CRDT sync), this method
    /// calculates the minimal diff between current and new content, applying
    /// only the necessary insert/delete operations. This ensures that Y.js
    /// operation IDs are preserved where content hasn't changed, allowing
    /// proper CRDT merging across clients.
    ///
    /// # Errors
    ///
    /// Returns an error if the update fails to persist to storage.
    pub fn set_body(&self, content: &str) -> StorageResult<()> {
        let doc_name = self.doc_name.read().unwrap().clone();
        log::trace!(
            "[BodyDoc] set_body called for '{}', content_len={}",
            doc_name,
            content.len()
        );

        // Get current content and state vector before the change
        let (current, sv_before) = {
            let txn = self.doc.transact();
            (self.body_text.get_string(&txn), txn.state_vector())
        };

        // If content is the same, no-op
        if current == content {
            log::trace!(
                "[BodyDoc] set_body: content unchanged for '{}', no-op",
                doc_name
            );
            return Ok(());
        }
        log::trace!(
            "[BodyDoc] set_body: content changed for '{}', current_len={}, new_len={}",
            doc_name,
            current.len(),
            content.len()
        );

        // Calculate minimal diff using common prefix/suffix approach
        let current_chars: Vec<char> = current.chars().collect();
        let new_chars: Vec<char> = content.chars().collect();

        // Find common prefix length
        let common_prefix = current_chars
            .iter()
            .zip(new_chars.iter())
            .take_while(|(a, b)| a == b)
            .count();

        // Find common suffix length (but don't overlap with prefix)
        let remaining_current = current_chars.len() - common_prefix;
        let remaining_new = new_chars.len() - common_prefix;
        let common_suffix = current_chars[common_prefix..]
            .iter()
            .rev()
            .zip(new_chars[common_prefix..].iter().rev())
            .take_while(|(a, b)| a == b)
            .take(remaining_current.min(remaining_new))
            .count();

        // Calculate the range to delete and text to insert
        let delete_start = common_prefix;
        let delete_end = current_chars.len() - common_suffix;
        let insert_start = common_prefix;
        let insert_end = new_chars.len() - common_suffix;

        // Apply the minimal changes
        {
            let mut txn = self.doc.transact_mut();

            // Delete the changed portion (if any)
            if delete_end > delete_start {
                // Y.js uses byte offsets, so convert char positions to Y.js positions
                // For TextRef, we need the length in Y.js units
                let delete_len = (delete_end - delete_start) as u32;
                self.body_text
                    .remove_range(&mut txn, delete_start as u32, delete_len);
            }

            // Insert the new portion (if any)
            if insert_end > insert_start {
                let insert_text: String = new_chars[insert_start..insert_end].iter().collect();
                self.body_text
                    .insert(&mut txn, delete_start as u32, &insert_text);
            }
        }

        // Capture the incremental update and store it
        self.record_update(&sv_before)
    }

    /// Insert text at a specific position.
    /// The change is automatically recorded in the update history.
    ///
    /// # Errors
    ///
    /// Returns an error if the update fails to persist to storage.
    pub fn insert_at(&self, index: u32, text: &str) -> StorageResult<()> {
        let sv_before = {
            let txn = self.doc.transact();
            txn.state_vector()
        };

        {
            let mut txn = self.doc.transact_mut();
            self.body_text.insert(&mut txn, index, text);
        }

        self.record_update(&sv_before)
    }

    /// Delete a range of text.
    /// The change is automatically recorded in the update history.
    ///
    /// # Errors
    ///
    /// Returns an error if the update fails to persist to storage.
    pub fn delete_range(&self, index: u32, length: u32) -> StorageResult<()> {
        let sv_before = {
            let txn = self.doc.transact();
            txn.state_vector()
        };

        {
            let mut txn = self.doc.transact_mut();
            self.body_text.remove_range(&mut txn, index, length);
        }

        self.record_update(&sv_before)
    }

    /// Helper to record an update in storage after a mutation.
    fn record_update(&self, sv_before: &yrs::StateVector) -> StorageResult<()> {
        let update = {
            let txn = self.doc.transact();
            txn.encode_state_as_update_v1(sv_before)
        };

        if !update.is_empty() {
            let doc_name = self.doc_name.read().unwrap();
            self.storage
                .append_update(&doc_name, &update, UpdateOrigin::Local)?;
        }
        Ok(())
    }

    /// Get the length of the body content.
    pub fn body_len(&self) -> u32 {
        let txn = self.doc.transact();
        self.body_text.len(&txn)
    }

    // ==================== Frontmatter Operations ====================

    /// Get a frontmatter property value as a string.
    pub fn get_frontmatter(&self, key: &str) -> Option<String> {
        let txn = self.doc.transact();
        self.frontmatter_map
            .get(&txn, key)
            .and_then(|v| v.cast::<String>().ok())
    }

    /// Set a frontmatter property.
    /// The change is automatically recorded in the update history.
    ///
    /// # Errors
    ///
    /// Returns an error if the update fails to persist to storage.
    pub fn set_frontmatter(&self, key: &str, value: &str) -> StorageResult<()> {
        let sv_before = {
            let txn = self.doc.transact();
            txn.state_vector()
        };

        {
            let mut txn = self.doc.transact_mut();
            self.frontmatter_map.insert(&mut txn, key, value);
        }

        self.record_update(&sv_before)
    }

    /// Remove a frontmatter property.
    /// The change is automatically recorded in the update history.
    ///
    /// # Errors
    ///
    /// Returns an error if the update fails to persist to storage.
    pub fn remove_frontmatter(&self, key: &str) -> StorageResult<()> {
        let sv_before = {
            let txn = self.doc.transact();
            txn.state_vector()
        };

        {
            let mut txn = self.doc.transact_mut();
            self.frontmatter_map.remove(&mut txn, key);
        }

        self.record_update(&sv_before)
    }

    /// Get all frontmatter keys.
    pub fn frontmatter_keys(&self) -> Vec<String> {
        let txn = self.doc.transact();
        self.frontmatter_map.keys(&txn).map(String::from).collect()
    }

    // ==================== Sync Operations ====================

    /// Encode the current state vector for sync.
    pub fn encode_state_vector(&self) -> Vec<u8> {
        let txn = self.doc.transact();
        txn.state_vector().encode_v1()
    }

    /// Encode the full state as an update.
    pub fn encode_state_as_update(&self) -> Vec<u8> {
        let txn = self.doc.transact();
        txn.encode_state_as_update_v1(&Default::default())
    }

    /// Encode the diff between current state and a remote state vector.
    pub fn encode_diff(&self, remote_state_vector: &[u8]) -> StorageResult<Vec<u8>> {
        let sv = yrs::StateVector::decode_v1(remote_state_vector)
            .map_err(|e| DiaryxError::Crdt(format!("Failed to decode state vector: {}", e)))?;
        let txn = self.doc.transact();
        Ok(txn.encode_state_as_update_v1(&sv))
    }

    /// Apply an update from a remote peer.
    ///
    /// For non-Local origins (Remote, Sync), this method will emit a `ContentsChanged`
    /// event via the event callback. This enables unified event handling where the UI
    /// responds the same way to both local and remote changes.
    ///
    /// This method also sets the `applying_remote` flag to prevent the update observer
    /// from firing, which would otherwise echo the remote update back to the server.
    pub fn apply_update(&self, update: &[u8], origin: UpdateOrigin) -> StorageResult<Option<i64>> {
        // Set flag to prevent observer from firing for remote updates
        // This prevents echoing back updates we receive from the server
        let is_remote = origin != UpdateOrigin::Local;
        if is_remote {
            self.applying_remote.store(true, Ordering::SeqCst);
        }

        let decoded = Update::decode_v1(update)
            .map_err(|e| DiaryxError::Crdt(format!("Failed to decode update: {}", e)))?;

        let result = {
            let mut txn = self.doc.transact_mut();
            txn.apply_update(decoded)
                .map_err(|e| DiaryxError::Crdt(format!("Failed to apply update: {}", e)))
        };

        // Clear flag after applying (before potential error return)
        if is_remote {
            self.applying_remote.store(false, Ordering::SeqCst);
        }

        // Propagate any error from apply_update
        result?;

        // NOTE: We do NOT emit ContentsChanged here. The caller (sync_manager::handle_body_message)
        // is responsible for emitting a single ContentsChanged event after processing all updates.
        // Emitting here caused triple-notification loops that triggered editor auto-save cascades.

        // Persist the update
        let doc_name = self.doc_name.read().unwrap();
        let update_id = self.storage.append_update(&doc_name, update, origin)?;
        Ok(Some(update_id))
    }

    // ==================== Persistence ====================

    /// Save the current state to storage.
    pub fn save(&self) -> StorageResult<()> {
        let state = self.encode_state_as_update();
        let doc_name = self.doc_name.read().unwrap();
        self.storage.save_doc(&doc_name, &state)
    }

    /// Reload state from storage.
    pub fn reload(&mut self) -> StorageResult<()> {
        let doc_name = self.doc_name.read().unwrap().clone();
        if let Some(state) = self.storage.load_doc(&doc_name)?
            && let Ok(update) = Update::decode_v1(&state)
        {
            let mut txn = self.doc.transact_mut();
            if let Err(e) = txn.apply_update(update) {
                log::warn!("Failed to reload body doc {}: {}", doc_name, e);
            }
        }
        Ok(())
    }

    // ==================== History ====================

    /// Get the update history for this document.
    pub fn get_history(&self) -> StorageResult<Vec<super::types::CrdtUpdate>> {
        let doc_name = self.doc_name.read().unwrap();
        self.storage.get_all_updates(&doc_name)
    }

    /// Get updates since a given ID.
    pub fn get_updates_since(&self, since_id: i64) -> StorageResult<Vec<super::types::CrdtUpdate>> {
        let doc_name = self.doc_name.read().unwrap();
        self.storage.get_updates_since(&doc_name, since_id)
    }

    // ==================== Observers ====================

    /// Observe text changes in the body.
    ///
    /// The callback is called whenever the body text changes.
    /// It receives the transaction and text event.
    pub fn observe_body<F>(&self, callback: F) -> yrs::Subscription
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.body_text.observe(move |_txn, _event| {
            callback();
        })
    }

    /// Observe changes to the underlying document.
    pub fn observe_updates<F>(&self, callback: F) -> yrs::Subscription
    where
        F: Fn(&[u8]) + Send + Sync + 'static,
    {
        self.doc
            .observe_update_v1(move |_, event| {
                callback(&event.update);
            })
            .expect("Failed to observe document updates")
    }
}

impl std::fmt::Debug for BodyDoc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let doc_name = self.doc_name.read().unwrap();
        f.debug_struct("BodyDoc")
            .field("doc_name", &*doc_name)
            .field("body_len", &self.body_len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crdt::MemoryStorage;

    fn create_body_doc(name: &str) -> BodyDoc {
        let storage = Arc::new(MemoryStorage::new());
        BodyDoc::new(storage, name.to_string())
    }

    #[test]
    fn test_new_body_doc_is_empty() {
        let doc = create_body_doc("test.md");
        assert_eq!(doc.get_body(), "");
        assert_eq!(doc.body_len(), 0);
    }

    #[test]
    fn test_set_and_get_body() {
        let doc = create_body_doc("test.md");

        let content = "# Hello World\n\nThis is content.";
        doc.set_body(content).unwrap();
        assert_eq!(doc.get_body(), content);
        assert_eq!(doc.body_len(), content.len() as u32);
    }

    #[test]
    fn test_replace_body() {
        let doc = create_body_doc("test.md");

        doc.set_body("Original content").unwrap();
        doc.set_body("New content").unwrap();

        assert_eq!(doc.get_body(), "New content");
    }

    #[test]
    fn test_insert_at() {
        let doc = create_body_doc("test.md");

        doc.set_body("Hello World").unwrap();
        doc.insert_at(6, "Beautiful ").unwrap();

        assert_eq!(doc.get_body(), "Hello Beautiful World");
    }

    #[test]
    fn test_delete_range() {
        let doc = create_body_doc("test.md");

        doc.set_body("Hello Beautiful World").unwrap();
        doc.delete_range(6, 10).unwrap(); // Remove "Beautiful "

        assert_eq!(doc.get_body(), "Hello World");
    }

    #[test]
    fn test_frontmatter_operations() {
        let doc = create_body_doc("test.md");

        // Set properties
        doc.set_frontmatter("title", "My Title").unwrap();
        doc.set_frontmatter("author", "John Doe").unwrap();

        // Get properties
        assert_eq!(doc.get_frontmatter("title"), Some("My Title".to_string()));
        assert_eq!(doc.get_frontmatter("author"), Some("John Doe".to_string()));
        assert_eq!(doc.get_frontmatter("nonexistent"), None);

        // List keys
        let keys = doc.frontmatter_keys();
        assert!(keys.contains(&"title".to_string()));
        assert!(keys.contains(&"author".to_string()));

        // Remove property
        doc.remove_frontmatter("author").unwrap();
        assert_eq!(doc.get_frontmatter("author"), None);
    }

    #[test]
    fn test_save_and_load() {
        let storage = Arc::new(MemoryStorage::new());
        let doc_name = "test.md".to_string();

        // Create and populate
        {
            let doc = BodyDoc::new(storage.clone(), doc_name.clone());
            doc.set_body("# Persistent Content").unwrap();
            doc.set_frontmatter("title", "Saved Title").unwrap();
            doc.save().unwrap();
        }

        // Load and verify
        {
            let doc = BodyDoc::load(storage, doc_name).unwrap();
            assert_eq!(doc.get_body(), "# Persistent Content");
            assert_eq!(
                doc.get_frontmatter("title"),
                Some("Saved Title".to_string())
            );
        }
    }

    #[test]
    fn test_sync_between_docs() {
        let storage1 = Arc::new(MemoryStorage::new());
        let storage2 = Arc::new(MemoryStorage::new());

        let doc1 = BodyDoc::new(storage1, "test.md".to_string());
        let doc2 = BodyDoc::new(storage2, "test.md".to_string());

        // Edit on doc1
        doc1.set_body("Content from doc1").unwrap();
        doc1.set_frontmatter("source", "doc1").unwrap();

        // Sync to doc2
        let update = doc1.encode_state_as_update();
        doc2.apply_update(&update, UpdateOrigin::Remote).unwrap();

        // Verify sync
        assert_eq!(doc2.get_body(), "Content from doc1");
        assert_eq!(doc2.get_frontmatter("source"), Some("doc1".to_string()));
    }

    #[test]
    fn test_concurrent_edits() {
        let storage1 = Arc::new(MemoryStorage::new());
        let storage2 = Arc::new(MemoryStorage::new());

        let doc1 = BodyDoc::new(storage1, "test.md".to_string());
        let doc2 = BodyDoc::new(storage2, "test.md".to_string());

        // Both start with same content
        doc1.set_body("Hello World").unwrap();
        let initial = doc1.encode_state_as_update();
        doc2.apply_update(&initial, UpdateOrigin::Remote).unwrap();

        // Concurrent edits
        doc1.insert_at(0, "A: ").unwrap(); // "A: Hello World"
        doc2.insert_at(11, "!").unwrap(); // "Hello World!"

        // Exchange updates
        let update1 = doc1.encode_state_as_update();
        let update2 = doc2.encode_state_as_update();

        doc1.apply_update(&update2, UpdateOrigin::Remote).unwrap();
        doc2.apply_update(&update1, UpdateOrigin::Remote).unwrap();

        // Both should converge to same result
        assert_eq!(doc1.get_body(), doc2.get_body());
        // Result should contain both edits
        let body = doc1.get_body();
        assert!(body.contains("A: "));
        assert!(body.contains("!"));
    }

    #[test]
    fn test_encode_diff() {
        let storage1 = Arc::new(MemoryStorage::new());
        let storage2 = Arc::new(MemoryStorage::new());

        let doc1 = BodyDoc::new(storage1, "test.md".to_string());
        let doc2 = BodyDoc::new(storage2, "test.md".to_string());

        // Initial sync
        doc1.set_body("Initial content").unwrap();
        let initial = doc1.encode_state_as_update();
        doc2.apply_update(&initial, UpdateOrigin::Remote).unwrap();

        // Doc2 captures state vector
        let sv2 = doc2.encode_state_vector();

        // Doc1 makes more changes
        doc1.insert_at(0, "NEW: ").unwrap();

        // Get only the diff
        let diff = doc1.encode_diff(&sv2).unwrap();

        // Apply diff to doc2
        doc2.apply_update(&diff, UpdateOrigin::Remote).unwrap();

        assert_eq!(doc2.get_body(), "NEW: Initial content");
    }

    #[test]
    fn test_observer_fires_on_change() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let doc = create_body_doc("test.md");
        let changed = Arc::new(AtomicBool::new(false));
        let changed_clone = changed.clone();

        let _sub = doc.observe_updates(move |_update| {
            changed_clone.store(true, Ordering::SeqCst);
        });

        doc.set_body("Trigger change").unwrap();

        assert!(changed.load(Ordering::SeqCst));
    }

    #[test]
    fn test_doc_name() {
        let doc = create_body_doc("workspace/notes/hello.md");
        assert_eq!(doc.doc_name(), "workspace/notes/hello.md");
    }

    #[test]
    fn test_sync_callback_uses_updated_doc_name_after_rename() {
        use std::sync::{Arc, Mutex};

        let doc = create_body_doc("old-name.md");
        let emitted_names = Arc::new(Mutex::new(Vec::<String>::new()));
        let emitted_names_clone = Arc::clone(&emitted_names);

        doc.set_sync_callback(Arc::new(move |doc_name: &str, _update: &[u8]| {
            emitted_names_clone
                .lock()
                .unwrap()
                .push(doc_name.to_string());
        }));

        doc.set_body("v1").unwrap();
        doc.set_doc_name("new-name.md".to_string());
        doc.set_body("v2").unwrap();

        let names = emitted_names.lock().unwrap();
        assert!(!names.is_empty(), "expected sync callbacks to fire");
        assert!(names.iter().any(|name| name == "old-name.md"));
        assert_eq!(names.last().map(String::as_str), Some("new-name.md"));
    }
}
