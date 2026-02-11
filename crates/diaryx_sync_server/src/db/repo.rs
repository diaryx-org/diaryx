use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use std::sync::{Arc, Mutex};

/// User information
#[derive(Debug, Clone)]
pub struct UserInfo {
    pub id: String,
    pub email: String,
    pub created_at: DateTime<Utc>,
    pub last_login_at: Option<DateTime<Utc>>,
}

/// Device information
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub id: String,
    pub user_id: String,
    pub name: Option<String>,
    pub user_agent: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
}

/// Session information
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub token: String,
    pub user_id: String,
    pub device_id: String,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

/// Workspace information
#[derive(Debug, Clone)]
pub struct WorkspaceInfo {
    pub id: String,
    pub user_id: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

/// Share session information
#[derive(Debug, Clone)]
pub struct ShareSessionInfo {
    pub code: String,
    pub workspace_id: String,
    pub owner_user_id: String,
    pub read_only: bool,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

/// Attachment reference entry for workspace reconciliation.
#[derive(Debug, Clone)]
pub struct WorkspaceAttachmentRefRecord {
    pub file_path: String,
    pub attachment_path: String,
    pub blob_hash: String,
    pub size_bytes: u64,
    pub mime_type: String,
}

/// Per-user storage usage summary.
#[derive(Debug, Clone, Default)]
pub struct UserStorageUsage {
    pub used_bytes: u64,
    pub blob_count: usize,
}

/// Blob row due for physical deletion.
#[derive(Debug, Clone)]
pub struct DueBlobDelete {
    pub user_id: String,
    pub blob_hash: String,
    pub r2_key: String,
}

/// Attachment upload session state.
#[derive(Debug, Clone)]
pub struct AttachmentUploadSession {
    pub upload_id: String,
    pub workspace_id: String,
    pub user_id: String,
    pub blob_hash: String,
    pub attachment_path: String,
    pub mime_type: String,
    pub size_bytes: u64,
    pub part_size: u64,
    pub total_parts: u32,
    pub r2_key: String,
    pub r2_multipart_upload_id: String,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub expires_at: i64,
}

/// Uploaded part metadata for multipart completion.
#[derive(Debug, Clone)]
pub struct AttachmentUploadPart {
    pub part_no: u32,
    pub etag: String,
    pub size_bytes: u64,
}

/// Authentication repository for database operations
#[derive(Clone)]
pub struct AuthRepo {
    conn: Arc<Mutex<Connection>>,
}

impl AuthRepo {
    /// Create a new AuthRepo with the given connection
    pub fn new(conn: Connection) -> Self {
        Self {
            conn: Arc::new(Mutex::new(conn)),
        }
    }

    // ===== User operations =====

    /// Get a user by ID
    pub fn get_user(&self, user_id: &str) -> Result<Option<UserInfo>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, email, created_at, last_login_at FROM users WHERE id = ?",
            [user_id],
            |row| {
                Ok(UserInfo {
                    id: row.get(0)?,
                    email: row.get(1)?,
                    created_at: timestamp_to_datetime(row.get(2)?),
                    last_login_at: row.get::<_, Option<i64>>(3)?.map(timestamp_to_datetime),
                })
            },
        )
        .optional()
    }

    /// Get a user by email
    pub fn get_user_by_email(&self, email: &str) -> Result<Option<UserInfo>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, email, created_at, last_login_at FROM users WHERE email = ?",
            [email],
            |row| {
                Ok(UserInfo {
                    id: row.get(0)?,
                    email: row.get(1)?,
                    created_at: timestamp_to_datetime(row.get(2)?),
                    last_login_at: row.get::<_, Option<i64>>(3)?.map(timestamp_to_datetime),
                })
            },
        )
        .optional()
    }

    /// Create or get a user by email (returns user ID)
    pub fn get_or_create_user(&self, email: &str) -> Result<String, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();

        // Try to get existing user
        if let Some(user_id) = conn
            .query_row("SELECT id FROM users WHERE email = ?", [email], |row| {
                row.get::<_, String>(0)
            })
            .optional()?
        {
            return Ok(user_id);
        }

        // Create new user
        let user_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().timestamp();

        conn.execute(
            "INSERT INTO users (id, email, created_at) VALUES (?, ?, ?)",
            params![user_id, email, now],
        )?;

        Ok(user_id)
    }

    /// Update user's last login time
    pub fn update_last_login(&self, user_id: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();
        conn.execute(
            "UPDATE users SET last_login_at = ? WHERE id = ?",
            params![now, user_id],
        )?;
        Ok(())
    }

    /// Delete a user and all related data (devices, sessions, workspaces, share_sessions cascade)
    /// Returns the list of workspace IDs that were deleted (for file cleanup)
    pub fn delete_user(&self, user_id: &str) -> Result<Vec<String>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();

        // First, get the user's workspace IDs for file cleanup
        let mut stmt = conn.prepare("SELECT id FROM user_workspaces WHERE user_id = ?")?;
        let workspace_ids: Vec<String> = stmt
            .query_map([user_id], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        // Delete the user (CASCADE will handle devices, sessions, workspaces, share_sessions)
        conn.execute("DELETE FROM users WHERE id = ?", [user_id])?;

        Ok(workspace_ids)
    }

    // ===== Device operations =====

    /// Create a new device
    pub fn create_device(
        &self,
        user_id: &str,
        name: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<String, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let device_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().timestamp();

        conn.execute(
            "INSERT INTO devices (id, user_id, name, user_agent, created_at, last_seen_at) VALUES (?, ?, ?, ?, ?, ?)",
            params![device_id, user_id, name, user_agent, now, now],
        )?;

        Ok(device_id)
    }

    /// Get devices for a user
    pub fn get_user_devices(&self, user_id: &str) -> Result<Vec<DeviceInfo>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, user_id, name, user_agent, created_at, last_seen_at
             FROM devices WHERE user_id = ? ORDER BY last_seen_at DESC",
        )?;

        let devices = stmt
            .query_map([user_id], |row| {
                Ok(DeviceInfo {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    name: row.get(2)?,
                    user_agent: row.get(3)?,
                    created_at: timestamp_to_datetime(row.get(4)?),
                    last_seen_at: timestamp_to_datetime(row.get(5)?),
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(devices)
    }

    /// Update device last seen time
    pub fn update_device_last_seen(&self, device_id: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();
        conn.execute(
            "UPDATE devices SET last_seen_at = ? WHERE id = ?",
            params![now, device_id],
        )?;
        Ok(())
    }

    /// Delete a device (and its sessions)
    pub fn delete_device(&self, device_id: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM devices WHERE id = ?", [device_id])?;
        Ok(())
    }

    // ===== Magic link operations =====

    /// Create a magic link token
    pub fn create_magic_token(
        &self,
        email: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<String, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();

        // Generate secure random token
        let token = generate_secure_token();
        let now = Utc::now().timestamp();

        conn.execute(
            "INSERT INTO magic_tokens (token, email, expires_at, created_at) VALUES (?, ?, ?, ?)",
            params![token, email, expires_at.timestamp(), now],
        )?;

        Ok(token)
    }

    /// Verify and consume a magic token (returns email if valid)
    pub fn verify_magic_token(&self, token: &str) -> Result<Option<String>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();

        // Get token if valid and not used
        let result: Option<String> = conn
            .query_row(
                "SELECT email FROM magic_tokens WHERE token = ? AND used = 0 AND expires_at > ?",
                params![token, now],
                |row| row.get(0),
            )
            .optional()?;

        if result.is_some() {
            // Mark token as used
            conn.execute("UPDATE magic_tokens SET used = 1 WHERE token = ?", [token])?;
        }

        Ok(result)
    }

    /// Clean up expired magic tokens
    pub fn cleanup_expired_magic_tokens(&self) -> Result<usize, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();
        let deleted = conn.execute("DELETE FROM magic_tokens WHERE expires_at < ?", [now])?;
        Ok(deleted)
    }

    /// Count recent magic tokens for an email (for rate limiting)
    pub fn count_recent_magic_tokens(
        &self,
        email: &str,
        since: DateTime<Utc>,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM magic_tokens WHERE email = ? AND created_at > ?",
            params![email, since.timestamp()],
            |row| row.get(0),
        )
    }

    // ===== Session operations =====

    /// Create a new auth session
    pub fn create_session(
        &self,
        user_id: &str,
        device_id: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<String, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let token = generate_secure_token();
        let now = Utc::now().timestamp();

        conn.execute(
            "INSERT INTO auth_sessions (token, user_id, device_id, expires_at, created_at) VALUES (?, ?, ?, ?, ?)",
            params![token, user_id, device_id, expires_at.timestamp(), now],
        )?;

        Ok(token)
    }

    /// Validate a session token (returns session info if valid)
    pub fn validate_session(&self, token: &str) -> Result<Option<SessionInfo>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();

        conn.query_row(
            "SELECT token, user_id, device_id, expires_at, created_at
             FROM auth_sessions WHERE token = ? AND expires_at > ?",
            params![token, now],
            |row| {
                Ok(SessionInfo {
                    token: row.get(0)?,
                    user_id: row.get(1)?,
                    device_id: row.get(2)?,
                    expires_at: timestamp_to_datetime(row.get(3)?),
                    created_at: timestamp_to_datetime(row.get(4)?),
                })
            },
        )
        .optional()
    }

    /// Delete a session
    pub fn delete_session(&self, token: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM auth_sessions WHERE token = ?", [token])?;
        Ok(())
    }

    /// Delete all sessions for a user
    pub fn delete_user_sessions(&self, user_id: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM auth_sessions WHERE user_id = ?", [user_id])?;
        Ok(())
    }

    /// Clean up expired sessions
    pub fn cleanup_expired_sessions(&self) -> Result<usize, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();
        let deleted = conn.execute("DELETE FROM auth_sessions WHERE expires_at < ?", [now])?;
        Ok(deleted)
    }

    // ===== Workspace operations =====

    /// Get or create a workspace for a user
    pub fn get_or_create_workspace(
        &self,
        user_id: &str,
        name: &str,
    ) -> Result<String, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();

        // Try to get existing workspace
        if let Some(workspace_id) = conn
            .query_row(
                "SELECT id FROM user_workspaces WHERE user_id = ? AND name = ?",
                params![user_id, name],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        {
            return Ok(workspace_id);
        }

        // Create new workspace
        let workspace_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().timestamp();

        conn.execute(
            "INSERT INTO user_workspaces (id, user_id, name, created_at) VALUES (?, ?, ?, ?)",
            params![workspace_id, user_id, name, now],
        )?;

        Ok(workspace_id)
    }

    /// Get all workspaces for a user
    pub fn get_user_workspaces(
        &self,
        user_id: &str,
    ) -> Result<Vec<WorkspaceInfo>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, user_id, name, created_at FROM user_workspaces WHERE user_id = ?",
        )?;

        let workspaces = stmt
            .query_map([user_id], |row| {
                Ok(WorkspaceInfo {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    name: row.get(2)?,
                    created_at: timestamp_to_datetime(row.get(3)?),
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(workspaces)
    }

    /// Get a workspace by ID
    pub fn get_workspace(
        &self,
        workspace_id: &str,
    ) -> Result<Option<WorkspaceInfo>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, user_id, name, created_at FROM user_workspaces WHERE id = ?",
            [workspace_id],
            |row| {
                Ok(WorkspaceInfo {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    name: row.get(2)?,
                    created_at: timestamp_to_datetime(row.get(3)?),
                })
            },
        )
        .optional()
    }

    // ===== Attachment blob accounting =====

    /// Insert or update a blob metadata row for a user.
    pub fn upsert_blob(
        &self,
        user_id: &str,
        blob_hash: &str,
        r2_key: &str,
        size_bytes: u64,
        mime_type: &str,
    ) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();
        conn.execute(
            "INSERT INTO user_attachment_blobs
             (user_id, blob_hash, r2_key, size_bytes, mime_type, ref_count, soft_deleted_at, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, 0, NULL, ?, ?)
             ON CONFLICT(user_id, blob_hash) DO UPDATE SET
               r2_key = excluded.r2_key,
               size_bytes = excluded.size_bytes,
               mime_type = excluded.mime_type,
               updated_at = excluded.updated_at",
            params![
                user_id,
                blob_hash,
                r2_key,
                size_bytes as i64,
                mime_type,
                now,
                now
            ],
        )?;
        Ok(())
    }

    /// Increment reference count for a user blob.
    pub fn inc_blob_ref(&self, user_id: &str, blob_hash: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();
        conn.execute(
            "UPDATE user_attachment_blobs
             SET ref_count = ref_count + 1, soft_deleted_at = NULL, updated_at = ?
             WHERE user_id = ? AND blob_hash = ?",
            params![now, user_id, blob_hash],
        )?;
        Ok(())
    }

    /// Decrement reference count for a user blob and mark soft delete when it reaches zero.
    pub fn dec_blob_ref(&self, user_id: &str, blob_hash: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();
        conn.execute(
            "UPDATE user_attachment_blobs
             SET ref_count = CASE WHEN ref_count > 0 THEN ref_count - 1 ELSE 0 END,
                 soft_deleted_at = CASE WHEN ref_count <= 1 THEN ? ELSE NULL END,
                 updated_at = ?
             WHERE user_id = ? AND blob_hash = ?",
            params![now, now, user_id, blob_hash],
        )?;
        Ok(())
    }

    fn get_workspace_user_id(
        conn: &Connection,
        workspace_id: &str,
    ) -> Result<Option<String>, rusqlite::Error> {
        conn.query_row(
            "SELECT user_id FROM user_workspaces WHERE id = ?",
            [workspace_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
    }

    /// Replace all attachment refs for a workspace and reconcile blob ref counts.
    pub fn replace_workspace_attachment_refs(
        &self,
        workspace_id: &str,
        refs: &[WorkspaceAttachmentRefRecord],
    ) -> Result<(), rusqlite::Error> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let now = Utc::now().timestamp();

        let user_id = match Self::get_workspace_user_id(&tx, workspace_id)? {
            Some(u) => u,
            None => return Ok(()),
        };

        let mut old_stmt = tx.prepare(
            "SELECT file_path, attachment_path, blob_hash
             FROM workspace_attachment_refs
             WHERE workspace_id = ?",
        )?;
        let old_rows: Vec<(String, String, String)> = old_stmt
            .query_map([workspace_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();
        drop(old_stmt);

        let mut old_map = std::collections::HashMap::new();
        for (file_path, attachment_path, blob_hash) in old_rows {
            old_map.insert((file_path, attachment_path), blob_hash);
        }

        let mut new_keys: std::collections::HashSet<(String, String)> =
            std::collections::HashSet::new();

        for entry in refs {
            let key = (entry.file_path.clone(), entry.attachment_path.clone());
            new_keys.insert(key.clone());

            tx.execute(
                "INSERT INTO user_attachment_blobs
                 (user_id, blob_hash, r2_key, size_bytes, mime_type, ref_count, soft_deleted_at, created_at, updated_at)
                 VALUES (?, ?, '', ?, ?, 0, NULL, ?, ?)
                 ON CONFLICT(user_id, blob_hash) DO UPDATE SET
                   size_bytes = excluded.size_bytes,
                   mime_type = excluded.mime_type,
                   updated_at = excluded.updated_at",
                params![
                    user_id,
                    entry.blob_hash,
                    entry.size_bytes as i64,
                    entry.mime_type,
                    now,
                    now
                ],
            )?;

            match old_map.get(&key) {
                Some(existing_hash) if existing_hash == &entry.blob_hash => {}
                Some(existing_hash) => {
                    tx.execute(
                        "UPDATE user_attachment_blobs
                         SET ref_count = CASE WHEN ref_count > 0 THEN ref_count - 1 ELSE 0 END,
                             soft_deleted_at = CASE WHEN ref_count <= 1 THEN ? ELSE soft_deleted_at END,
                             updated_at = ?
                         WHERE user_id = ? AND blob_hash = ?",
                        params![now, now, user_id, existing_hash],
                    )?;
                    tx.execute(
                        "UPDATE user_attachment_blobs
                         SET ref_count = ref_count + 1, soft_deleted_at = NULL, updated_at = ?
                         WHERE user_id = ? AND blob_hash = ?",
                        params![now, user_id, entry.blob_hash],
                    )?;
                }
                None => {
                    tx.execute(
                        "UPDATE user_attachment_blobs
                         SET ref_count = ref_count + 1, soft_deleted_at = NULL, updated_at = ?
                         WHERE user_id = ? AND blob_hash = ?",
                        params![now, user_id, entry.blob_hash],
                    )?;
                }
            }

            tx.execute(
                "INSERT INTO workspace_attachment_refs
                 (workspace_id, file_path, attachment_path, blob_hash, size_bytes, mime_type, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(workspace_id, file_path, attachment_path) DO UPDATE SET
                   blob_hash = excluded.blob_hash,
                   size_bytes = excluded.size_bytes,
                   mime_type = excluded.mime_type,
                   updated_at = excluded.updated_at",
                params![
                    workspace_id,
                    entry.file_path,
                    entry.attachment_path,
                    entry.blob_hash,
                    entry.size_bytes as i64,
                    entry.mime_type,
                    now
                ],
            )?;
        }

        for ((file_path, attachment_path), blob_hash) in old_map {
            if new_keys.contains(&(file_path.clone(), attachment_path.clone())) {
                continue;
            }

            tx.execute(
                "DELETE FROM workspace_attachment_refs
                 WHERE workspace_id = ? AND file_path = ? AND attachment_path = ?",
                params![workspace_id, file_path, attachment_path],
            )?;

            tx.execute(
                "UPDATE user_attachment_blobs
                 SET ref_count = CASE WHEN ref_count > 0 THEN ref_count - 1 ELSE 0 END,
                     soft_deleted_at = CASE WHEN ref_count <= 1 THEN ? ELSE soft_deleted_at END,
                     updated_at = ?
                 WHERE user_id = ? AND blob_hash = ?",
                params![now, now, user_id, blob_hash],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    /// Get per-user attachment usage (active references only).
    pub fn get_user_storage_usage(
        &self,
        user_id: &str,
    ) -> Result<UserStorageUsage, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT COALESCE(SUM(size_bytes), 0), COALESCE(COUNT(*), 0)
             FROM user_attachment_blobs
             WHERE user_id = ? AND ref_count > 0",
        )?;
        let usage = stmt.query_row([user_id], |row| {
            Ok(UserStorageUsage {
                used_bytes: row.get::<_, i64>(0)? as u64,
                blob_count: row.get::<_, i64>(1)? as usize,
            })
        })?;
        Ok(usage)
    }

    /// List blobs that are soft deleted and due for physical deletion.
    pub fn list_soft_deleted_blobs_due(
        &self,
        due_before_ts: i64,
    ) -> Result<Vec<DueBlobDelete>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT user_id, blob_hash, r2_key
             FROM user_attachment_blobs
             WHERE ref_count = 0
               AND soft_deleted_at IS NOT NULL
               AND soft_deleted_at <= ?",
        )?;
        let rows = stmt
            .query_map([due_before_ts], |row| {
                Ok(DueBlobDelete {
                    user_id: row.get(0)?,
                    blob_hash: row.get(1)?,
                    r2_key: row.get(2)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    /// Delete a blob row by primary key.
    pub fn delete_blob_row(&self, user_id: &str, blob_hash: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM user_attachment_blobs WHERE user_id = ? AND blob_hash = ?",
            params![user_id, blob_hash],
        )?;
        Ok(())
    }

    /// List all blob keys for a user (for account deletion cleanup).
    pub fn list_user_blob_keys(&self, user_id: &str) -> Result<Vec<String>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT r2_key FROM user_attachment_blobs WHERE user_id = ?")?;
        let keys = stmt
            .query_map([user_id], |row| row.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(keys)
    }

    /// Get blob metadata for a user/hash.
    pub fn get_user_blob(
        &self,
        user_id: &str,
        blob_hash: &str,
    ) -> Result<Option<(String, u64, String)>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT r2_key, size_bytes, mime_type
             FROM user_attachment_blobs
             WHERE user_id = ? AND blob_hash = ?",
            params![user_id, blob_hash],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)? as u64,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
    }

    /// Check whether a workspace currently references a blob hash.
    pub fn workspace_references_blob(
        &self,
        workspace_id: &str,
        blob_hash: &str,
    ) -> Result<bool, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM workspace_attachment_refs WHERE workspace_id = ? AND blob_hash = ?",
            params![workspace_id, blob_hash],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Create or replace a multipart upload session.
    pub fn create_attachment_upload_session(
        &self,
        session: &AttachmentUploadSession,
    ) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO attachment_uploads
             (upload_id, workspace_id, user_id, blob_hash, attachment_path, mime_type, size_bytes, part_size, total_parts, r2_key, r2_multipart_upload_id, status, created_at, updated_at, expires_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(upload_id) DO UPDATE SET
               workspace_id = excluded.workspace_id,
               user_id = excluded.user_id,
               blob_hash = excluded.blob_hash,
               attachment_path = excluded.attachment_path,
               mime_type = excluded.mime_type,
               size_bytes = excluded.size_bytes,
               part_size = excluded.part_size,
               total_parts = excluded.total_parts,
               r2_key = excluded.r2_key,
               r2_multipart_upload_id = excluded.r2_multipart_upload_id,
               status = excluded.status,
               updated_at = excluded.updated_at,
               expires_at = excluded.expires_at",
            params![
                session.upload_id,
                session.workspace_id,
                session.user_id,
                session.blob_hash,
                session.attachment_path,
                session.mime_type,
                session.size_bytes as i64,
                session.part_size as i64,
                session.total_parts as i64,
                session.r2_key,
                session.r2_multipart_upload_id,
                session.status,
                session.created_at,
                session.updated_at,
                session.expires_at
            ],
        )?;
        Ok(())
    }

    /// Get multipart upload session by upload ID.
    pub fn get_attachment_upload_session(
        &self,
        upload_id: &str,
    ) -> Result<Option<AttachmentUploadSession>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT upload_id, workspace_id, user_id, blob_hash, attachment_path, mime_type, size_bytes, part_size, total_parts, r2_key, r2_multipart_upload_id, status, created_at, updated_at, expires_at
             FROM attachment_uploads
             WHERE upload_id = ?",
            [upload_id],
            |row| {
                Ok(AttachmentUploadSession {
                    upload_id: row.get(0)?,
                    workspace_id: row.get(1)?,
                    user_id: row.get(2)?,
                    blob_hash: row.get(3)?,
                    attachment_path: row.get(4)?,
                    mime_type: row.get(5)?,
                    size_bytes: row.get::<_, i64>(6)? as u64,
                    part_size: row.get::<_, i64>(7)? as u64,
                    total_parts: row.get::<_, i64>(8)? as u32,
                    r2_key: row.get(9)?,
                    r2_multipart_upload_id: row.get(10)?,
                    status: row.get(11)?,
                    created_at: row.get(12)?,
                    updated_at: row.get(13)?,
                    expires_at: row.get(14)?,
                })
            },
        )
        .optional()
    }

    /// Upsert uploaded part metadata for a multipart session.
    pub fn upsert_attachment_upload_part(
        &self,
        upload_id: &str,
        part_no: u32,
        etag: &str,
        size_bytes: u64,
    ) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();
        conn.execute(
            "INSERT INTO attachment_upload_parts (upload_id, part_no, etag, size_bytes, created_at)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(upload_id, part_no) DO UPDATE SET
               etag = excluded.etag,
               size_bytes = excluded.size_bytes",
            params![upload_id, part_no as i64, etag, size_bytes as i64, now],
        )?;
        conn.execute(
            "UPDATE attachment_uploads SET updated_at = ? WHERE upload_id = ?",
            params![now, upload_id],
        )?;
        Ok(())
    }

    /// List uploaded parts for a multipart session in ascending part order.
    pub fn list_attachment_upload_parts(
        &self,
        upload_id: &str,
    ) -> Result<Vec<AttachmentUploadPart>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT part_no, etag, size_bytes
             FROM attachment_upload_parts
             WHERE upload_id = ?
             ORDER BY part_no ASC",
        )?;
        let parts = stmt
            .query_map([upload_id], |row| {
                Ok(AttachmentUploadPart {
                    part_no: row.get::<_, i64>(0)? as u32,
                    etag: row.get(1)?,
                    size_bytes: row.get::<_, i64>(2)? as u64,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(parts)
    }

    /// Set status for a multipart upload session.
    pub fn set_attachment_upload_status(
        &self,
        upload_id: &str,
        status: &str,
    ) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();
        conn.execute(
            "UPDATE attachment_uploads SET status = ?, updated_at = ? WHERE upload_id = ?",
            params![status, now, upload_id],
        )?;
        Ok(())
    }

    /// List sessions that have expired while still uploading.
    pub fn list_expired_attachment_uploads(
        &self,
        now_ts: i64,
    ) -> Result<Vec<AttachmentUploadSession>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT upload_id, workspace_id, user_id, blob_hash, attachment_path, mime_type, size_bytes, part_size, total_parts, r2_key, r2_multipart_upload_id, status, created_at, updated_at, expires_at
             FROM attachment_uploads
             WHERE status = 'uploading' AND expires_at <= ?",
        )?;
        let rows = stmt
            .query_map([now_ts], |row| {
                Ok(AttachmentUploadSession {
                    upload_id: row.get(0)?,
                    workspace_id: row.get(1)?,
                    user_id: row.get(2)?,
                    blob_hash: row.get(3)?,
                    attachment_path: row.get(4)?,
                    mime_type: row.get(5)?,
                    size_bytes: row.get::<_, i64>(6)? as u64,
                    part_size: row.get::<_, i64>(7)? as u64,
                    total_parts: row.get::<_, i64>(8)? as u32,
                    r2_key: row.get(9)?,
                    r2_multipart_upload_id: row.get(10)?,
                    status: row.get(11)?,
                    created_at: row.get(12)?,
                    updated_at: row.get(13)?,
                    expires_at: row.get(14)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    /// Delete multipart session and all associated part rows.
    pub fn delete_attachment_upload_session(&self, upload_id: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM attachment_uploads WHERE upload_id = ?",
            params![upload_id],
        )?;
        Ok(())
    }

    // ===== Share session operations =====

    /// Create a new share session
    pub fn create_share_session(
        &self,
        workspace_id: &str,
        owner_user_id: &str,
        read_only: bool,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<String, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let code = generate_session_code();
        let now = Utc::now().timestamp();

        conn.execute(
            "INSERT INTO share_sessions (code, workspace_id, owner_user_id, read_only, created_at, expires_at) VALUES (?, ?, ?, ?, ?, ?)",
            params![code, workspace_id, owner_user_id, read_only as i32, now, expires_at.map(|e| e.timestamp())],
        )?;

        Ok(code)
    }

    /// Get a share session by code
    pub fn get_share_session(
        &self,
        code: &str,
    ) -> Result<Option<ShareSessionInfo>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();

        conn.query_row(
            "SELECT code, workspace_id, owner_user_id, read_only, created_at, expires_at
             FROM share_sessions
             WHERE code = ? AND (expires_at IS NULL OR expires_at > ?)",
            params![code, now],
            |row| {
                Ok(ShareSessionInfo {
                    code: row.get(0)?,
                    workspace_id: row.get(1)?,
                    owner_user_id: row.get(2)?,
                    read_only: row.get::<_, i32>(3)? != 0,
                    created_at: timestamp_to_datetime(row.get(4)?),
                    expires_at: row.get::<_, Option<i64>>(5)?.map(timestamp_to_datetime),
                })
            },
        )
        .optional()
    }

    /// Get all share sessions for a user
    pub fn get_user_share_sessions(
        &self,
        user_id: &str,
    ) -> Result<Vec<ShareSessionInfo>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();

        let mut stmt = conn.prepare(
            "SELECT code, workspace_id, owner_user_id, read_only, created_at, expires_at
             FROM share_sessions
             WHERE owner_user_id = ? AND (expires_at IS NULL OR expires_at > ?)
             ORDER BY created_at DESC",
        )?;

        let sessions = stmt
            .query_map(params![user_id, now], |row| {
                Ok(ShareSessionInfo {
                    code: row.get(0)?,
                    workspace_id: row.get(1)?,
                    owner_user_id: row.get(2)?,
                    read_only: row.get::<_, i32>(3)? != 0,
                    created_at: timestamp_to_datetime(row.get(4)?),
                    expires_at: row.get::<_, Option<i64>>(5)?.map(timestamp_to_datetime),
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(sessions)
    }

    /// Update read-only status for a share session
    pub fn update_share_session_read_only(
        &self,
        code: &str,
        read_only: bool,
    ) -> Result<bool, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let updated = conn.execute(
            "UPDATE share_sessions SET read_only = ? WHERE code = ?",
            params![read_only as i32, code],
        )?;
        Ok(updated > 0)
    }

    /// Delete a share session
    pub fn delete_share_session(&self, code: &str) -> Result<bool, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let deleted = conn.execute("DELETE FROM share_sessions WHERE code = ?", [code])?;
        Ok(deleted > 0)
    }

    /// Clean up expired share sessions
    pub fn cleanup_expired_share_sessions(&self) -> Result<usize, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();
        let deleted = conn.execute(
            "DELETE FROM share_sessions WHERE expires_at IS NOT NULL AND expires_at < ?",
            [now],
        )?;
        Ok(deleted)
    }
}

// ===== Helper functions =====

/// Generate a cryptographically secure random token
fn generate_secure_token() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.r#gen()).collect();
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, bytes)
}

/// Generate a session code in XXXXXXXX-XXXXXXXX format
fn generate_session_code() -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();

    let part1: String = (0..8)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect();

    let part2: String = (0..8)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect();

    format!("{}-{}", part1, part2)
}

/// Convert Unix timestamp to DateTime<Utc>
fn timestamp_to_datetime(timestamp: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(timestamp, 0).unwrap_or_else(|| Utc::now())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_database;

    fn setup_test_db() -> AuthRepo {
        let conn = Connection::open_in_memory().unwrap();
        init_database(&conn).unwrap();
        AuthRepo::new(conn)
    }

    #[test]
    fn test_user_creation() {
        let repo = setup_test_db();

        let user_id = repo.get_or_create_user("test@example.com").unwrap();
        assert!(!user_id.is_empty());

        // Getting the same user should return the same ID
        let user_id2 = repo.get_or_create_user("test@example.com").unwrap();
        assert_eq!(user_id, user_id2);

        // Verify user exists
        let user = repo.get_user(&user_id).unwrap();
        assert!(user.is_some());
        assert_eq!(user.unwrap().email, "test@example.com");
    }

    #[test]
    fn test_magic_token_flow() {
        let repo = setup_test_db();
        let email = "test@example.com";
        let expires = Utc::now() + chrono::Duration::hours(1);

        // Create token
        let token = repo.create_magic_token(email, expires).unwrap();
        assert!(!token.is_empty());

        // Verify token
        let verified_email = repo.verify_magic_token(&token).unwrap();
        assert_eq!(verified_email, Some(email.to_string()));

        // Token should be consumed (can't verify again)
        let second_verify = repo.verify_magic_token(&token).unwrap();
        assert!(second_verify.is_none());
    }

    #[test]
    fn test_session_flow() {
        let repo = setup_test_db();

        // Create user and device
        let user_id = repo.get_or_create_user("test@example.com").unwrap();
        let device_id = repo
            .create_device(&user_id, Some("Test Device"), None)
            .unwrap();

        // Create session
        let expires = Utc::now() + chrono::Duration::days(30);
        let token = repo.create_session(&user_id, &device_id, expires).unwrap();

        // Validate session
        let session = repo.validate_session(&token).unwrap();
        assert!(session.is_some());
        let session = session.unwrap();
        assert_eq!(session.user_id, user_id);
        assert_eq!(session.device_id, device_id);

        // Delete session
        repo.delete_session(&token).unwrap();
        let deleted = repo.validate_session(&token).unwrap();
        assert!(deleted.is_none());
    }

    #[test]
    fn test_delete_user() {
        let repo = setup_test_db();

        // Create user with device, session, and workspace
        let user_id = repo.get_or_create_user("delete-test@example.com").unwrap();
        let device_id = repo
            .create_device(&user_id, Some("Test Device"), None)
            .unwrap();
        let expires = Utc::now() + chrono::Duration::days(30);
        let session_token = repo.create_session(&user_id, &device_id, expires).unwrap();
        let workspace_id = repo.get_or_create_workspace(&user_id, "default").unwrap();

        // Verify everything exists
        assert!(repo.get_user(&user_id).unwrap().is_some());
        assert!(!repo.get_user_devices(&user_id).unwrap().is_empty());
        assert!(repo.validate_session(&session_token).unwrap().is_some());
        assert!(!repo.get_user_workspaces(&user_id).unwrap().is_empty());

        // Delete user
        let deleted_workspace_ids = repo.delete_user(&user_id).unwrap();
        assert_eq!(deleted_workspace_ids, vec![workspace_id]);

        // Verify everything is deleted (cascade)
        assert!(repo.get_user(&user_id).unwrap().is_none());
        assert!(repo.get_user_devices(&user_id).unwrap().is_empty());
        assert!(repo.validate_session(&session_token).unwrap().is_none());
        assert!(repo.get_user_workspaces(&user_id).unwrap().is_empty());
    }

    #[test]
    fn test_attachment_blob_accounting() {
        let repo = setup_test_db();
        let user_id = repo.get_or_create_user("blob-test@example.com").unwrap();
        let workspace_id = repo.get_or_create_workspace(&user_id, "default").unwrap();

        repo.upsert_blob(
            &user_id,
            "hash-a",
            "diaryx-sync/u/blob-test/blobs/hash-a",
            1024,
            "image/png",
        )
        .unwrap();
        repo.upsert_blob(
            &user_id,
            "hash-b",
            "diaryx-sync/u/blob-test/blobs/hash-b",
            2048,
            "application/pdf",
        )
        .unwrap();

        repo.replace_workspace_attachment_refs(
            &workspace_id,
            &[WorkspaceAttachmentRefRecord {
                file_path: "README.md".to_string(),
                attachment_path: "_attachments/a.png".to_string(),
                blob_hash: "hash-a".to_string(),
                size_bytes: 1024,
                mime_type: "image/png".to_string(),
            }],
        )
        .unwrap();

        let usage = repo.get_user_storage_usage(&user_id).unwrap();
        assert_eq!(usage.used_bytes, 1024);
        assert_eq!(usage.blob_count, 1);

        // Two refs to same blob still count once in storage usage.
        repo.replace_workspace_attachment_refs(
            &workspace_id,
            &[
                WorkspaceAttachmentRefRecord {
                    file_path: "README.md".to_string(),
                    attachment_path: "_attachments/a.png".to_string(),
                    blob_hash: "hash-a".to_string(),
                    size_bytes: 1024,
                    mime_type: "image/png".to_string(),
                },
                WorkspaceAttachmentRefRecord {
                    file_path: "notes.md".to_string(),
                    attachment_path: "_attachments/a-copy.png".to_string(),
                    blob_hash: "hash-a".to_string(),
                    size_bytes: 1024,
                    mime_type: "image/png".to_string(),
                },
            ],
        )
        .unwrap();

        let usage = repo.get_user_storage_usage(&user_id).unwrap();
        assert_eq!(usage.used_bytes, 1024);
        assert_eq!(usage.blob_count, 1);

        // Replacing with a different blob updates usage.
        repo.replace_workspace_attachment_refs(
            &workspace_id,
            &[WorkspaceAttachmentRefRecord {
                file_path: "README.md".to_string(),
                attachment_path: "_attachments/b.pdf".to_string(),
                blob_hash: "hash-b".to_string(),
                size_bytes: 2048,
                mime_type: "application/pdf".to_string(),
            }],
        )
        .unwrap();

        let usage = repo.get_user_storage_usage(&user_id).unwrap();
        assert_eq!(usage.used_bytes, 2048);
        assert_eq!(usage.blob_count, 1);

        // Clearing refs soft-deletes blobs.
        repo.replace_workspace_attachment_refs(&workspace_id, &[])
            .unwrap();
        let usage = repo.get_user_storage_usage(&user_id).unwrap();
        assert_eq!(usage.used_bytes, 0);
        assert_eq!(usage.blob_count, 0);

        let due = repo.list_soft_deleted_blobs_due(i64::MAX).unwrap();
        assert!(!due.is_empty());
    }
}
