//! Unified sync manager for workspace and body synchronization.
//!
//! This module provides `RustSyncManager`, which replaces all TypeScript sync bridges
//! (bodySyncBridge.ts, rustSyncBridge.ts) with a single unified Rust implementation.
//!
//! # Responsibilities
//!
//! - Workspace metadata sync (replaces rustSyncBridge.ts)
//! - Per-file body sync (replaces bodySyncBridge.ts)
//! - Sync completion tracking (replaces TS debounce logic)
//! - Echo detection (replaces lastKnownBodyContent Map)
//!
//! # Usage
//!
//! ```ignore
//! let manager = RustSyncManager::new(workspace_crdt, body_manager, sync_handler);
//!
//! // Handle incoming workspace message
//! let (response, synced) = manager.handle_workspace_message(&msg, true).await?;
//!
//! // Handle incoming body message
//! let (response, content_changed) = manager.handle_body_message("path.md", &msg, true).await?;
//! ```

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use super::body_doc_manager::BodyDocManager;
use super::sync::SyncMessage;
use super::sync_handler::SyncHandler;
use super::types::{FileMetadata, UpdateOrigin};
use super::workspace_doc::WorkspaceCrdt;
use crate::error::Result;
use crate::fs::{AsyncFileSystem, FileSystemEvent};

/// Result of handling a sync message.
#[derive(Debug)]
pub struct SyncMessageResult {
    /// Optional response bytes to send back to the server.
    pub response: Option<Vec<u8>>,
    /// List of file paths that were changed by this message.
    pub changed_files: Vec<String>,
    /// Whether sync is now complete (for initial sync tracking).
    pub sync_complete: bool,
}

/// Result of handling a body sync message.
#[derive(Debug)]
pub struct BodySyncResult {
    /// Optional response bytes to send back to the server.
    pub response: Option<Vec<u8>>,
    /// New content if it changed, None if unchanged.
    pub content: Option<String>,
    /// Whether this is an echo of our own update.
    pub is_echo: bool,
}

/// Unified sync manager for workspace and body synchronization.
///
/// This struct replaces all TypeScript sync bridges with a single unified
/// Rust implementation. It handles:
/// - Workspace metadata sync via Y-sync protocol
/// - Per-file body sync via Y-sync protocol
/// - Sync completion tracking
/// - Echo detection to avoid processing our own updates
/// - File locking to prevent concurrent modifications
pub struct RustSyncManager<FS: AsyncFileSystem> {
    // Core CRDT components
    workspace_crdt: Arc<WorkspaceCrdt>,
    body_manager: Arc<BodyDocManager>,
    sync_handler: Arc<SyncHandler<FS>>,

    // Workspace sync state
    workspace_synced: AtomicBool,
    workspace_message_count: Mutex<u32>,

    // Per-file body sync tracking (which docs have completed initial sync)
    body_synced: RwLock<HashSet<String>>,

    // Echo detection - tracks last known content to detect our own updates
    last_known_content: RwLock<HashMap<String, String>>,

    // Metadata echo detection - tracks last known metadata to detect our own updates
    last_known_metadata: RwLock<HashMap<String, FileMetadata>>,

    // Last sent state vector per body doc (for delta encoding).
    // This tracks what we've already sent so we only send new changes.
    last_sent_body_sv: RwLock<HashMap<String, Vec<u8>>>,

    // Initial sync tracking
    initial_sync_complete: AtomicBool,

    // Callback to emit filesystem events (for SendSyncMessage)
    event_callback: RwLock<Option<Arc<dyn Fn(&FileSystemEvent) + Send + Sync>>>,

    // Cached state vectors after last successful sync (SyncStep2 received).
    // Used to skip sending SyncStep1 when local state hasn't changed since last sync.
    last_synced_workspace_sv: RwLock<Option<Vec<u8>>>,
    last_synced_body_svs: RwLock<HashMap<String, Vec<u8>>>,

    // Files this client is focused on (for focus-based sync).
    // Used to track which files to re-focus on reconnect.
    focused_files: RwLock<HashSet<String>>,
}

impl<FS: AsyncFileSystem> RustSyncManager<FS> {
    /// Create a new sync manager.
    pub fn new(
        workspace_crdt: Arc<WorkspaceCrdt>,
        body_manager: Arc<BodyDocManager>,
        sync_handler: Arc<SyncHandler<FS>>,
    ) -> Self {
        Self {
            workspace_crdt,
            body_manager,
            sync_handler,
            workspace_synced: AtomicBool::new(false),
            workspace_message_count: Mutex::new(0),
            body_synced: RwLock::new(HashSet::new()),
            last_known_content: RwLock::new(HashMap::new()),
            last_known_metadata: RwLock::new(HashMap::new()),
            last_sent_body_sv: RwLock::new(HashMap::new()),
            initial_sync_complete: AtomicBool::new(false),
            event_callback: RwLock::new(None),
            last_synced_workspace_sv: RwLock::new(None),
            last_synced_body_svs: RwLock::new(HashMap::new()),
            focused_files: RwLock::new(HashSet::new()),
        }
    }

    /// Set the event callback for emitting filesystem events.
    ///
    /// This callback is used to emit SendSyncMessage events to TypeScript,
    /// which then sends the bytes over WebSocket to the sync server.
    ///
    /// **Important**: This also sets up the body sync observer callback on the body_manager,
    /// so that local body changes automatically emit sync messages via the Yrs observer pattern.
    pub fn set_event_callback(&self, callback: Arc<dyn Fn(&FileSystemEvent) + Send + Sync>) {
        log::debug!("[SyncManager] set_event_callback called, setting up body sync observer");
        {
            let mut cb = self.event_callback.write().unwrap();
            *cb = Some(callback.clone());
        }

        // Set up body sync callback using the Yrs observer pattern.
        // When body docs are mutated locally, the observer automatically emits
        // the update bytes as a sync message.
        self.setup_body_sync_observer(callback);
    }

    /// Set up the body sync observer callback.
    ///
    /// This uses the Yrs observer pattern: when any body doc is mutated locally,
    /// the observer automatically receives the exact update bytes and emits them
    /// as a sync message via the event callback.
    fn setup_body_sync_observer(
        &self,
        event_callback: Arc<dyn Fn(&FileSystemEvent) + Send + Sync>,
    ) {
        let sync_callback = Arc::new(move |doc_name: &str, update: &[u8]| {
            log::debug!(
                "[SyncManager] Body observer: doc='{}', update_len={}",
                doc_name,
                update.len()
            );

            // Wrap the update in a SyncMessage and emit via the event callback
            let message = SyncMessage::Update(update.to_vec()).encode();
            let event = FileSystemEvent::send_sync_message(doc_name, message, true);
            event_callback(&event);
        });

        self.body_manager.set_sync_callback(sync_callback);
    }

    /// Clear the event callback.
    ///
    /// Call this when stopping sync to prevent sending to a disconnected channel.
    pub fn clear_event_callback(&self) {
        let mut cb = self.event_callback.write().unwrap();
        *cb = None;
    }

    /// Emit a filesystem event via the callback.
    fn emit_event(&self, event: FileSystemEvent) {
        if let Some(ref cb) = *self.event_callback.read().unwrap() {
            cb(&event);
        }
    }

    /// Create and emit a workspace sync message.
    ///
    /// Call this after updating the workspace CRDT to send the changes
    /// to the sync server via TypeScript WebSocket.
    pub fn emit_workspace_update(&self) -> Result<()> {
        let update = self.create_workspace_update(None)?;
        if !update.is_empty() {
            log::debug!(
                "[SyncManager] emit_workspace_update: sending {} bytes",
                update.len()
            );
            self.emit_event(FileSystemEvent::send_sync_message(
                "workspace",
                update,
                false,
            ));
        } else {
            log::debug!("[SyncManager] emit_workspace_update: update is empty, nothing to send");
        }
        Ok(())
    }

    /// Create and emit a body sync message.
    ///
    /// Call this after updating a body CRDT to send the changes
    /// to the sync server via TypeScript WebSocket.
    ///
    /// IMPORTANT: This assumes the body CRDT has already been updated via set_body().
    /// It only encodes the current state - it does NOT call set_body() again.
    ///
    /// The `doc_name` is the canonical file path (e.g., "workspace/notes.md").
    /// The `content` is used only for echo detection tracking.
    ///
    /// This method uses delta encoding: it tracks the last-sent state vector
    /// and only sends changes since then, not the full document state.
    pub fn emit_body_update(&self, doc_name: &str, content: &str) -> Result<()> {
        log::debug!(
            "[SyncManager] emit_body_update: doc_name='{}', content_preview='{}'",
            doc_name,
            content.chars().take(50).collect::<String>()
        );

        // Track for echo detection (don't update CRDT - it's already updated)
        {
            let mut last_known = self.last_known_content.write().unwrap();
            last_known.insert(doc_name.to_string(), content.to_string());
        }

        // Get the body doc (should already exist and have content set)
        let body_doc = self.body_manager.get_or_create(doc_name);

        // Use delta encoding: only send changes since last sent state vector.
        // This is more efficient than sending the full state every time.
        let update = {
            let sv_map = self.last_sent_body_sv.read().unwrap();
            if let Some(last_sv) = sv_map.get(doc_name) {
                // We have a previous state vector, send only the diff
                body_doc
                    .encode_diff(last_sv)
                    .unwrap_or_else(|_| body_doc.encode_state_as_update())
            } else {
                // First time sending for this doc, send full state
                body_doc.encode_state_as_update()
            }
        };

        if update.is_empty() {
            log::debug!(
                "[SyncManager] emit_body_update: update is empty for doc_name='{}'",
                doc_name
            );
            return Ok(());
        }

        // Store new state vector for next delta calculation
        {
            let new_sv = body_doc.encode_state_vector();
            let mut sv_map = self.last_sent_body_sv.write().unwrap();
            sv_map.insert(doc_name.to_string(), new_sv);
        }

        log::debug!(
            "[SyncManager] emit_body_update: sending {} bytes (delta) for doc_name='{}'",
            update.len(),
            doc_name
        );
        let message = SyncMessage::Update(update).encode();
        self.emit_event(FileSystemEvent::send_sync_message(doc_name, message, true));
        Ok(())
    }

    // =========================================================================
    // Workspace Sync
    // =========================================================================

    /// Handle an incoming WebSocket message for workspace sync.
    ///
    /// Returns a `SyncMessageResult` containing:
    /// - Optional response bytes to send back
    /// - List of changed file paths
    /// - Whether sync is now complete
    ///
    /// If `write_to_disk` is true, changed files will be written to disk
    /// via the SyncHandler.
    pub async fn handle_workspace_message(
        &self,
        message: &[u8],
        write_to_disk: bool,
    ) -> Result<SyncMessageResult> {
        log::debug!(
            "[SyncManager] handle_workspace_message: {} bytes, write_to_disk: {}",
            message.len(),
            write_to_disk
        );

        // Decode all messages in the buffer
        let messages = SyncMessage::decode_all(message)?;
        if messages.is_empty() {
            log::debug!("[SyncManager] No messages decoded");
            return Ok(SyncMessageResult {
                response: None,
                changed_files: Vec::new(),
                sync_complete: false,
            });
        }

        let mut response: Option<Vec<u8>> = None;
        let mut all_changed_files: Vec<String> = Vec::new();
        let mut all_renames: Vec<(String, String)> = Vec::new();

        for sync_msg in messages {
            let (msg_response, changed_files, renames) =
                self.handle_single_workspace_message(sync_msg).await?;

            all_changed_files.extend(changed_files);
            all_renames.extend(renames);

            // Combine responses
            if let Some(resp) = msg_response {
                if let Some(ref mut existing) = response {
                    existing.extend_from_slice(&resp);
                } else {
                    response = Some(resp);
                }
            }
        }

        // Filter metadata echoes and dedupe paths before emitting change events or syncing to disk.
        let mut filtered_changed_files: Vec<String> = Vec::new();
        if !all_changed_files.is_empty() {
            let mut seen = HashSet::new();
            for path in &all_changed_files {
                if !seen.insert(path.clone()) {
                    continue;
                }

                match self.workspace_crdt.get_file(path) {
                    Some(meta) => {
                        if self.is_metadata_echo(path, &meta) {
                            log::debug!("[SyncManager] Skipping metadata echo for: {}", path);
                            continue;
                        }
                    }
                    None => {
                        // Keep paths that no longer exist in the CRDT (e.g., deletes).
                    }
                }

                filtered_changed_files.push(path.clone());
            }
        }

        // Write changed files to disk if requested
        if write_to_disk && (!filtered_changed_files.is_empty() || !all_renames.is_empty()) {
            let files_to_sync: Vec<_> = filtered_changed_files
                .iter()
                .filter_map(|path| {
                    let meta = self.workspace_crdt.get_file(path);
                    log::info!(
                        "[SyncManager] get_file for sync '{}': exists={}, deleted={:?}",
                        path,
                        meta.is_some(),
                        meta.as_ref().map(|m| m.deleted)
                    );
                    meta.and_then(|m| Some((path.clone(), m)))
                })
                .collect();

            if !files_to_sync.is_empty() || !all_renames.is_empty() {
                let body_mgr_ref = Some(self.body_manager.as_ref());
                self.sync_handler
                    .handle_remote_metadata_update(files_to_sync, all_renames, body_mgr_ref, true)
                    .await?;
            }
        }

        // Track message count for sync completion detection
        let mut count = self.workspace_message_count.lock().unwrap();
        *count += 1;

        // Consider synced after receiving at least one message
        // (The TypeScript version used a 300ms debounce, but we can track this more precisely)
        let sync_complete = !self.workspace_synced.swap(true, Ordering::SeqCst);
        if sync_complete {
            log::debug!("[SyncManager] Workspace sync complete");
            self.initial_sync_complete.store(true, Ordering::SeqCst);
        }

        Ok(SyncMessageResult {
            response,
            changed_files: filtered_changed_files,
            sync_complete,
        })
    }

    /// Handle a single workspace sync message.
    /// Returns (response, changed_files, renames).
    async fn handle_single_workspace_message(
        &self,
        msg: SyncMessage,
    ) -> Result<(Option<Vec<u8>>, Vec<String>, Vec<(String, String)>)> {
        match msg {
            SyncMessage::SyncStep1(remote_sv) => {
                log::debug!(
                    "[SyncManager] Workspace: Received SyncStep1, {} bytes",
                    remote_sv.len()
                );

                // Create SyncStep2 with our updates
                let diff = self.workspace_crdt.encode_diff(&remote_sv)?;
                let step2 = SyncMessage::SyncStep2(diff).encode();

                // Also send our state vector
                let our_sv = self.workspace_crdt.encode_state_vector();
                let step1 = SyncMessage::SyncStep1(our_sv).encode();

                let mut combined = step2;
                combined.extend_from_slice(&step1);

                Ok((Some(combined), Vec::new(), Vec::new()))
            }

            SyncMessage::SyncStep2(update) => {
                log::debug!(
                    "[SyncManager] Workspace: Received SyncStep2, {} bytes",
                    update.len()
                );

                let mut changed_files = Vec::new();
                let mut renames = Vec::new();
                if !update.is_empty() {
                    let (_, files, detected_renames) = self
                        .workspace_crdt
                        .apply_update_tracking_changes(&update, UpdateOrigin::Sync)?;
                    changed_files = files;
                    renames = detected_renames;
                }

                // Cache the new state vector after successful sync
                let new_sv = self.workspace_crdt.encode_state_vector();
                {
                    let mut cache = self.last_synced_workspace_sv.write().unwrap();
                    *cache = Some(new_sv);
                }

                Ok((None, changed_files, renames))
            }

            SyncMessage::Update(update) => {
                log::debug!(
                    "[SyncManager] Workspace: Received Update, {} bytes",
                    update.len()
                );

                let mut changed_files = Vec::new();
                let mut renames = Vec::new();
                if !update.is_empty() {
                    let (_, files, detected_renames) = self
                        .workspace_crdt
                        .apply_update_tracking_changes(&update, UpdateOrigin::Remote)?;
                    changed_files = files;
                    renames = detected_renames;
                }

                Ok((None, changed_files, renames))
            }
        }
    }

    /// Create a SyncStep1 message for workspace sync.
    pub fn create_workspace_sync_step1(&self) -> Vec<u8> {
        let sv = self.workspace_crdt.encode_state_vector();
        SyncMessage::SyncStep1(sv).encode()
    }

    /// Create an update message for local workspace changes.
    ///
    /// If `since_state_vector` is provided, returns only updates since that state.
    /// Otherwise returns the full state.
    pub fn create_workspace_update(&self, since_state_vector: Option<&[u8]>) -> Result<Vec<u8>> {
        let update = match since_state_vector {
            Some(sv) => self.workspace_crdt.encode_diff(sv)?,
            None => self.workspace_crdt.encode_state_as_update(),
        };

        if update.is_empty() {
            return Ok(Vec::new());
        }

        Ok(SyncMessage::Update(update).encode())
    }

    /// Check if workspace sync is complete.
    pub fn is_workspace_synced(&self) -> bool {
        self.workspace_synced.load(Ordering::SeqCst)
    }

    // =========================================================================
    // Body Sync
    // =========================================================================

    /// Initialize body sync for a document.
    ///
    /// Ensures the body document exists and is ready for sync.
    pub fn init_body_sync(&self, doc_name: &str) {
        // Ensure the body doc exists (loads from storage if available)
        let _ = self.body_manager.get_or_create(doc_name);
        log::debug!("[SyncManager] Initialized body sync for: {}", doc_name);
    }

    /// Close body sync for a document.
    pub fn close_body_sync(&self, doc_name: &str) {
        let mut synced = self.body_synced.write().unwrap();
        synced.remove(doc_name);

        // Also clear the last-sent state vector
        let mut sv_map = self.last_sent_body_sv.write().unwrap();
        sv_map.remove(doc_name);

        log::debug!("[SyncManager] Closed body sync for: {}", doc_name);
    }

    /// Handle an incoming WebSocket message for body sync.
    ///
    /// Returns a `BodySyncResult` containing:
    /// - Optional response bytes to send back
    /// - New content if it changed
    /// - Whether this is an echo of our own update
    pub async fn handle_body_message(
        &self,
        doc_name: &str,
        message: &[u8],
        write_to_disk: bool,
    ) -> Result<BodySyncResult> {
        log::info!(
            "[SyncManager] handle_body_message START: doc='{}', message_len={}, write_to_disk={}",
            doc_name,
            message.len(),
            write_to_disk
        );

        // Ensure body doc exists
        self.init_body_sync(doc_name);

        // Get the body doc - this is the SINGLE source of truth
        let body_doc = self.body_manager.get_or_create(doc_name);
        let content_before = body_doc.get_body();
        log::info!(
            "[SyncManager] handle_body_message: doc='{}', content_before_len={}",
            doc_name,
            content_before.len()
        );

        // Decode and process all messages, building response and applying updates
        let messages = SyncMessage::decode_all(message)?;
        log::info!(
            "[SyncManager] handle_body_message: doc='{}', decoded {} messages",
            doc_name,
            messages.len()
        );

        let mut response: Option<Vec<u8>> = None;

        for (i, sync_msg) in messages.iter().enumerate() {
            match sync_msg {
                SyncMessage::SyncStep1(remote_sv) => {
                    // Respond with SyncStep2 containing our diff based on their state vector
                    log::info!(
                        "[SyncManager] handle_body_message: doc='{}', msg[{}] = SyncStep1, sv_len={}",
                        doc_name,
                        i,
                        remote_sv.len()
                    );

                    // Generate SyncStep2 response using body_doc directly
                    if let Ok(diff) = body_doc.encode_diff(remote_sv) {
                        if diff.len() > 2 {
                            // More than just empty update header
                            let step2 = SyncMessage::SyncStep2(diff).encode();
                            log::info!(
                                "[SyncManager] handle_body_message: doc='{}', sending SyncStep2 response, {} bytes",
                                doc_name,
                                step2.len()
                            );
                            if let Some(ref mut existing) = response {
                                existing.extend_from_slice(&step2);
                            } else {
                                response = Some(step2);
                            }
                        }
                    }
                }
                SyncMessage::SyncStep2(update) | SyncMessage::Update(update) => {
                    let is_step2 = matches!(sync_msg, SyncMessage::SyncStep2(_));
                    log::info!(
                        "[SyncManager] handle_body_message: doc='{}', msg[{}] = {:?}, update_len={}",
                        doc_name,
                        i,
                        if is_step2 { "SyncStep2" } else { "Update" },
                        update.len()
                    );
                    if !update.is_empty() {
                        body_doc.apply_update(update, UpdateOrigin::Remote)?;
                    }

                    // Cache the new state vector after successful SyncStep2
                    if is_step2 {
                        let new_sv = body_doc.encode_state_vector();
                        let mut cache = self.last_synced_body_svs.write().unwrap();
                        cache.insert(doc_name.to_string(), new_sv);
                    }
                }
            }
        }

        let content_after = body_doc.get_body();
        log::info!(
            "[SyncManager] handle_body_message: doc='{}', content_after_len={}, content_after_preview='{}'",
            doc_name,
            content_after.len(),
            content_after.chars().take(100).collect::<String>()
        );

        // Check if content changed
        let content_changed = content_before != content_after;

        // Check if this is an echo of our own update
        let is_echo = if content_changed {
            let last_known = self.last_known_content.read().unwrap();
            let tracked_content = last_known.get(doc_name);
            let echo_check = tracked_content == Some(&content_after);
            log::info!(
                "[SyncManager] handle_body_message echo check: doc='{}', has_tracked_content={}, tracked_len={}, echo_check={}",
                doc_name,
                tracked_content.is_some(),
                tracked_content.map(|s| s.len()).unwrap_or(0),
                echo_check
            );
            echo_check
        } else {
            false
        };

        log::info!(
            "[SyncManager] handle_body_message RESULT: doc='{}', content_changed={}, is_echo={}, write_to_disk={}",
            doc_name,
            content_changed,
            is_echo,
            write_to_disk
        );

        // Write to disk if content changed and not an echo
        if write_to_disk && content_changed && !is_echo {
            // Get metadata from workspace CRDT if available
            let metadata = self.workspace_crdt.get_file(doc_name);
            self.sync_handler
                .handle_remote_body_update(doc_name, &content_after, metadata.as_ref())
                .await?;
        }

        // Notify UI of remote body change (even if write_to_disk is false, e.g., guest mode)
        if content_changed && !is_echo {
            let event =
                FileSystemEvent::contents_changed(PathBuf::from(doc_name), content_after.clone());
            self.emit_event(event);
        }

        // Mark as synced
        {
            let mut synced = self.body_synced.write().unwrap();
            synced.insert(doc_name.to_string());
        }

        // Update last sent state vector after receiving remote updates.
        // This ensures our next emit_body_update() will calculate deltas
        // from the correct baseline (including the remote changes we just received).
        {
            let new_sv = body_doc.encode_state_vector();
            let mut sv_map = self.last_sent_body_sv.write().unwrap();
            sv_map.insert(doc_name.to_string(), new_sv);
        }

        Ok(BodySyncResult {
            response,
            content: if content_changed && !is_echo {
                Some(content_after)
            } else {
                None
            },
            is_echo,
        })
    }

    /// Create a SyncStep1 message for body sync.
    pub fn create_body_sync_step1(&self, doc_name: &str) -> Vec<u8> {
        self.init_body_sync(doc_name);

        // Use body_doc directly - it's the single source of truth
        let body_doc = self.body_manager.get_or_create(doc_name);
        let sv = body_doc.encode_state_vector();
        SyncMessage::SyncStep1(sv).encode()
    }

    /// Ensure body content is populated from disk before sync.
    ///
    /// This method reads the file content from disk and sets it into the body CRDT.
    /// It should be called before `create_body_sync_step1()` to ensure the body
    /// CRDT has content to sync (rather than sending an empty state vector).
    ///
    /// Returns true if content was loaded, false if the body doc already had content
    /// or the file doesn't exist.
    pub async fn ensure_body_content_loaded(&self, doc_name: &str) -> Result<bool> {
        // Check if body doc already has content
        let body_doc = self.body_manager.get_or_create(doc_name);
        let existing_content = body_doc.get_body();

        if !existing_content.is_empty() {
            log::debug!(
                "[SyncManager] Body already has content for {}: {} chars",
                doc_name,
                existing_content.len()
            );
            return Ok(false);
        }

        // Check if file exists on disk
        if !self.sync_handler.file_exists(doc_name).await {
            log::debug!(
                "[SyncManager] File does not exist for body {}, skipping load",
                doc_name
            );
            return Ok(false);
        }

        // Read content from disk
        match self.sync_handler.read_body_content(doc_name).await {
            Ok(content) => {
                if content.is_empty() {
                    log::debug!(
                        "[SyncManager] File has empty body for {}, nothing to load",
                        doc_name
                    );
                    return Ok(false);
                }

                log::info!(
                    "[SyncManager] Loading body content from disk for {}: {} chars",
                    doc_name,
                    content.len()
                );

                // Set content into body CRDT
                body_doc.set_body(&content)?;

                // Track for echo detection
                {
                    let mut last_known = self.last_known_content.write().unwrap();
                    last_known.insert(doc_name.to_string(), content);
                }

                Ok(true)
            }
            Err(e) => {
                log::warn!(
                    "[SyncManager] Failed to read body content for {}: {:?}",
                    doc_name,
                    e
                );
                Ok(false)
            }
        }
    }

    /// Create an update message for local body changes.
    pub fn create_body_update(&self, doc_name: &str, content: &str) -> Result<Vec<u8>> {
        // Update content in body doc
        let body_doc = self.body_manager.get_or_create(doc_name);
        body_doc.set_body(content)?;

        // Track for echo detection
        {
            let mut last_known = self.last_known_content.write().unwrap();
            last_known.insert(doc_name.to_string(), content.to_string());
        }

        // Get full state as update
        let update = body_doc.encode_state_as_update();
        if update.is_empty() {
            return Ok(Vec::new());
        }

        Ok(SyncMessage::Update(update).encode())
    }

    /// Check if body sync is complete for a document.
    pub fn is_body_synced(&self, doc_name: &str) -> bool {
        let synced = self.body_synced.read().unwrap();
        synced.contains(doc_name)
    }

    // =========================================================================
    // Sync State Comparison (for skip-if-unchanged optimization)
    // =========================================================================

    /// Check if workspace state has changed since last successful sync.
    ///
    /// Returns true if:
    /// - This is the first sync (no cached state)
    /// - The local state vector differs from the cached state after last SyncStep2
    ///
    /// Used to skip sending SyncStep1 on reconnect when state hasn't changed.
    pub fn workspace_state_changed(&self) -> bool {
        let current_sv = self.workspace_crdt.encode_state_vector();
        let last_synced = self.last_synced_workspace_sv.read().unwrap();
        match &*last_synced {
            Some(sv) => current_sv != *sv,
            None => true, // First sync, always send
        }
    }

    /// Check if body doc state has changed since last successful sync.
    ///
    /// Returns true if:
    /// - This is the first sync for this doc (no cached state)
    /// - The doc is not loaded yet
    /// - The local state vector differs from the cached state after last SyncStep2
    ///
    /// Used to skip sending SyncStep1 on reconnect when state hasn't changed.
    pub fn body_state_changed(&self, doc_name: &str) -> bool {
        let body_doc = match self.body_manager.get(doc_name) {
            Some(doc) => doc,
            None => return true, // Doc not loaded, need to sync
        };
        let current_sv = body_doc.encode_state_vector();
        let last_synced = self.last_synced_body_svs.read().unwrap();
        match last_synced.get(doc_name) {
            Some(sv) => current_sv != *sv,
            None => true, // First sync for this doc
        }
    }

    // =========================================================================
    // Echo Detection
    // =========================================================================

    /// Check if content change is an echo of our own edit.
    pub fn is_echo(&self, path: &str, content: &str) -> bool {
        let last_known = self.last_known_content.read().unwrap();
        last_known.get(path) == Some(&content.to_string())
    }

    /// Track content for echo detection.
    pub fn track_content(&self, path: &str, content: &str) {
        let mut last_known = self.last_known_content.write().unwrap();
        last_known.insert(path.to_string(), content.to_string());
    }

    /// Clear tracked content (e.g., when closing a file).
    pub fn clear_tracked_content(&self, path: &str) {
        let mut last_known = self.last_known_content.write().unwrap();
        last_known.remove(path);
    }

    /// Check if metadata change is an echo of our own edit (ignoring modified_at).
    pub fn is_metadata_echo(&self, path: &str, metadata: &FileMetadata) -> bool {
        let last_known = self.last_known_metadata.read().unwrap();
        if let Some(known) = last_known.get(path) {
            // Use is_content_equal which compares all fields except modified_at
            known.is_content_equal(metadata)
        } else {
            false
        }
    }

    /// Track metadata for echo detection.
    pub fn track_metadata(&self, path: &str, metadata: &FileMetadata) {
        let mut last_known = self.last_known_metadata.write().unwrap();
        last_known.insert(path.to_string(), metadata.clone());
    }

    /// Clear tracked metadata (e.g., when closing a file).
    pub fn clear_tracked_metadata(&self, path: &str) {
        let mut last_known = self.last_known_metadata.write().unwrap();
        last_known.remove(path);
    }

    // =========================================================================
    // File Discovery
    // =========================================================================

    /// Get all active file paths in the workspace CRDT.
    ///
    /// Used by SyncClient to initiate body sync for all files after the body
    /// connection is established.
    pub fn get_all_file_paths(&self) -> Vec<String> {
        self.workspace_crdt
            .list_active_files()
            .into_iter()
            .map(|(path, _)| path)
            .collect()
    }

    // =========================================================================
    // Sync State
    // =========================================================================

    /// Mark initial sync as complete.
    pub fn mark_sync_complete(&self) {
        self.initial_sync_complete.store(true, Ordering::SeqCst);
        self.workspace_synced.store(true, Ordering::SeqCst);
        log::info!("[SyncManager] Initial sync marked complete");
    }

    /// Check if initial sync is complete.
    pub fn is_sync_complete(&self) -> bool {
        self.initial_sync_complete.load(Ordering::SeqCst)
    }

    /// Get list of body docs that have completed initial sync.
    pub fn get_active_syncs(&self) -> Vec<String> {
        let synced = self.body_synced.read().unwrap();
        synced.iter().cloned().collect()
    }

    // =========================================================================
    // Focus Tracking (for focus-based sync)
    // =========================================================================

    /// Set the files this client is focused on.
    ///
    /// This is used to track focus for reconnection - when the client reconnects,
    /// it should re-focus on these files.
    pub fn set_focused_files(&self, files: &[String]) {
        let mut focused = self.focused_files.write().unwrap();
        focused.clear();
        for file in files {
            focused.insert(file.clone());
        }
        log::debug!("[SyncManager] Set focused files: {:?}", files);
    }

    /// Get the files this client is focused on.
    ///
    /// Returns the list of files to re-focus on after reconnection.
    pub fn get_focused_files(&self) -> Vec<String> {
        let focused = self.focused_files.read().unwrap();
        focused.iter().cloned().collect()
    }

    /// Add files to the focus list.
    pub fn add_focused_files(&self, files: &[String]) {
        let mut focused = self.focused_files.write().unwrap();
        for file in files {
            focused.insert(file.clone());
        }
    }

    /// Remove files from the focus list.
    pub fn remove_focused_files(&self, files: &[String]) {
        let mut focused = self.focused_files.write().unwrap();
        for file in files {
            focused.remove(file);
        }
    }

    /// Check if a file is in the focus list.
    pub fn is_file_focused(&self, file: &str) -> bool {
        let focused = self.focused_files.read().unwrap();
        focused.contains(file)
    }

    // =========================================================================
    // Path Handling (delegates to SyncHandler)
    // =========================================================================

    /// Get the storage path for a canonical path.
    pub fn get_storage_path(&self, canonical_path: &str) -> PathBuf {
        self.sync_handler.get_storage_path(canonical_path)
    }

    /// Get the canonical path from a storage path.
    pub fn get_canonical_path(&self, storage_path: &str) -> String {
        self.sync_handler.get_canonical_path(storage_path)
    }

    /// Check if we're in guest mode.
    pub fn is_guest(&self) -> bool {
        self.sync_handler.is_guest()
    }

    // =========================================================================
    // Cleanup
    // =========================================================================

    // =========================================================================
    // Handshake Protocol (for preventing CRDT corruption on initial sync)
    // =========================================================================

    /// Handle the CrdtState message from the server's handshake protocol.
    ///
    /// This is called after the client has downloaded all files (via HTTP or
    /// batch request) and sent the `FilesReady` message. The server then
    /// sends the full CRDT state which is applied to the workspace.
    ///
    /// **Important**: This method is designed for new clients with empty workspaces.
    /// When the local workspace is empty, applying the server's full state via
    /// `apply_update` works correctly without tombstoning issues. The handshake
    /// protocol ensures files are downloaded BEFORE this state is applied, so
    /// the CRDT state and filesystem are consistent.
    ///
    /// # Arguments
    /// * `state` - The full CRDT state as bytes (Y-update v1 encoded)
    ///
    /// # Returns
    /// The number of files in the workspace after applying the state
    pub fn handle_crdt_state(&self, state: &[u8]) -> Result<usize> {
        log::info!(
            "[SyncManager] handle_crdt_state: applying {} bytes of state to workspace",
            state.len()
        );

        // Apply the state to the workspace CRDT
        // This works correctly for new clients because:
        // 1. The local workspace is empty (no files to tombstone)
        // 2. Files were already downloaded via the handshake
        // 3. The state contains all file metadata to match the filesystem
        self.workspace_crdt
            .apply_update_tracking_changes(state, UpdateOrigin::Sync)?;

        // Mark sync as complete since we now have the authoritative state
        self.mark_sync_complete();

        // Return the number of files in the workspace
        let files = self.workspace_crdt.list_active_files();
        log::info!(
            "[SyncManager] handle_crdt_state: workspace now has {} active files",
            files.len()
        );

        Ok(files.len())
    }

    /// Check if the workspace CRDT is empty (no files).
    ///
    /// Used to determine if this is a "new client" that needs the handshake
    /// protocol to prevent CRDT corruption.
    pub fn is_workspace_empty(&self) -> bool {
        self.workspace_crdt.list_active_files().is_empty()
    }

    // =========================================================================
    // Cleanup
    // =========================================================================

    /// Reset all sync state.
    pub fn reset(&self) {
        self.workspace_synced.store(false, Ordering::SeqCst);
        self.initial_sync_complete.store(false, Ordering::SeqCst);

        {
            let mut count = self.workspace_message_count.lock().unwrap();
            *count = 0;
        }

        {
            let mut synced = self.body_synced.write().unwrap();
            synced.clear();
        }

        {
            let mut last_known = self.last_known_content.write().unwrap();
            last_known.clear();
        }

        {
            let mut last_known = self.last_known_metadata.write().unwrap();
            last_known.clear();
        }

        {
            let mut sv_map = self.last_sent_body_sv.write().unwrap();
            sv_map.clear();
        }

        // Clear cached synced state vectors (for skip-if-unchanged optimization)
        {
            let mut cache = self.last_synced_workspace_sv.write().unwrap();
            *cache = None;
        }

        {
            let mut cache = self.last_synced_body_svs.write().unwrap();
            cache.clear();
        }

        // Note: We intentionally do NOT clear focused_files on reset.
        // Focus should persist across reconnections so the client can
        // re-focus on the same files after reconnecting.

        log::info!("[SyncManager] Reset complete");
    }
}

impl<FS: AsyncFileSystem> std::fmt::Debug for RustSyncManager<FS> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RustSyncManager")
            .field("workspace_synced", &self.workspace_synced)
            .field("initial_sync_complete", &self.initial_sync_complete)
            .field("active_body_syncs", &self.get_active_syncs().len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crdt::MemoryStorage;
    use crate::crdt::storage::CrdtStorage;
    use crate::fs::SyncToAsyncFs;
    use crate::test_utils::MockFileSystem;

    fn create_test_manager() -> RustSyncManager<SyncToAsyncFs<MockFileSystem>> {
        let storage: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());
        let workspace_crdt = Arc::new(WorkspaceCrdt::new(Arc::clone(&storage)));
        let body_manager = Arc::new(BodyDocManager::new(Arc::clone(&storage)));
        let fs = SyncToAsyncFs::new(MockFileSystem::new());
        let sync_handler = Arc::new(SyncHandler::new(fs));

        RustSyncManager::new(workspace_crdt, body_manager, sync_handler)
    }

    #[test]
    fn test_workspace_sync_step1() {
        let manager = create_test_manager();
        let step1 = manager.create_workspace_sync_step1();

        // Should be a valid SyncStep1 message
        assert!(!step1.is_empty());
        assert_eq!(step1[0], 0); // SYNC type
        assert_eq!(step1[1], 0); // STEP1 subtype
    }

    #[test]
    fn test_body_sync_init_and_close() {
        let manager = create_test_manager();

        // Initially no active syncs (syncs that have completed initial handshake)
        assert!(manager.get_active_syncs().is_empty());

        // init_body_sync creates the body doc but doesn't mark it as synced
        // (synced status is set by handle_body_message after receiving server response)
        manager.init_body_sync("test.md");
        assert!(manager.get_active_syncs().is_empty());

        // Simulate that a sync completed by directly adding to body_synced
        // (normally this happens in handle_body_message)
        {
            let mut synced = manager.body_synced.write().unwrap();
            synced.insert("test.md".to_string());
        }
        assert_eq!(manager.get_active_syncs(), vec!["test.md"]);

        // close_body_sync removes from synced set
        manager.close_body_sync("test.md");
        assert!(manager.get_active_syncs().is_empty());
    }

    #[test]
    fn test_echo_detection() {
        let manager = create_test_manager();

        // Track content
        manager.track_content("test.md", "Hello world");

        // Should detect echo
        assert!(manager.is_echo("test.md", "Hello world"));

        // Should not detect different content as echo
        assert!(!manager.is_echo("test.md", "Different content"));

        // Clear and check
        manager.clear_tracked_content("test.md");
        assert!(!manager.is_echo("test.md", "Hello world"));
    }

    #[test]
    fn test_metadata_echo_detection() {
        use crate::crdt::FileMetadata;

        let manager = create_test_manager();

        // Create metadata
        let mut meta = FileMetadata::new(Some("Test".to_string()));
        meta.part_of = Some("parent/index.md".to_string());

        // Track metadata
        manager.track_metadata("test.md", &meta);

        // Should detect echo with same content (even if modified_at differs)
        let mut meta2 = meta.clone();
        meta2.modified_at = 999999; // Different timestamp
        assert!(manager.is_metadata_echo("test.md", &meta2));

        // Should not detect different content as echo
        let mut meta3 = meta.clone();
        meta3.title = Some("Different".to_string());
        assert!(!manager.is_metadata_echo("test.md", &meta3));

        // Clear and check
        manager.clear_tracked_metadata("test.md");
        assert!(!manager.is_metadata_echo("test.md", &meta));
    }

    #[test]
    fn test_sync_state() {
        let manager = create_test_manager();

        // Initially not synced
        assert!(!manager.is_sync_complete());
        assert!(!manager.is_workspace_synced());

        // Mark complete
        manager.mark_sync_complete();
        assert!(manager.is_sync_complete());
        assert!(manager.is_workspace_synced());

        // Reset
        manager.reset();
        assert!(!manager.is_sync_complete());
        assert!(!manager.is_workspace_synced());
    }

    #[test]
    fn test_handle_crdt_state() {
        use super::FileMetadata;

        // Create a source workspace with files
        let source_storage: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());
        let source_workspace = WorkspaceCrdt::new(Arc::clone(&source_storage));

        // Add a file to the source workspace
        let meta = FileMetadata::new(Some("Test File".to_string()));
        source_workspace.create_file(meta).unwrap();

        // Encode the source workspace state
        let state = source_workspace.encode_state_as_update();

        // Create target manager (empty workspace)
        let manager = create_test_manager();

        // Initially workspace is empty
        assert!(manager.is_workspace_empty());
        assert!(!manager.is_sync_complete());

        // Handle the CRDT state (simulating server handshake)
        let result = manager.handle_crdt_state(&state);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 1); // Should have 1 file

        // Workspace should now be marked as synced
        assert!(manager.is_sync_complete());
    }

    #[test]
    fn test_is_workspace_empty() {
        use super::FileMetadata;

        // Create storage and workspace directly (not via manager) to test
        let storage: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());
        let workspace = WorkspaceCrdt::new(Arc::clone(&storage));

        // Initially empty
        assert!(workspace.list_active_files().is_empty());

        // Add a file to workspace
        let meta = FileMetadata::new(Some("Test".to_string()));
        workspace.create_file(meta).unwrap();

        // No longer empty
        assert!(!workspace.list_active_files().is_empty());
    }
}
