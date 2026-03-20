//! Namespace, object, audience, and usage repository methods.

use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use std::sync::{Arc, Mutex};

/// Namespace record.
#[derive(Debug, Clone)]
pub struct NamespaceInfo {
    pub id: String,
    pub owner_user_id: String,
    pub created_at: i64,
}

/// Metadata for an object in a namespace (excludes inline data).
#[derive(Debug, Clone)]
pub struct NamespaceObjectMeta {
    pub namespace_id: String,
    pub key: String,
    /// R2 object key, or `None` for inline objects.
    pub r2_key: Option<String>,
    pub mime_type: String,
    pub size_bytes: u64,
    pub updated_at: i64,
    /// Audience tag. `None` = private (owner-only).
    pub audience: Option<String>,
}

/// Audience visibility record.
#[derive(Debug, Clone)]
pub struct AudienceInfo {
    pub namespace_id: String,
    pub audience_name: String,
    pub access: String, // "public" | "token" | "private"
}

/// Session mapping: code → namespace.
#[derive(Debug, Clone)]
pub struct NamespaceSessionInfo {
    pub code: String,
    pub namespace_id: String,
    pub owner_user_id: String,
    pub read_only: bool,
    pub created_at: i64,
    pub expires_at: Option<i64>,
}

/// Aggregated usage totals for a user.
#[derive(Debug, Clone, Default)]
pub struct UsageTotals {
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub relay_seconds: u64,
}

/// Custom domain record mapping a domain to a namespace + audience.
#[derive(Debug, Clone)]
pub struct CustomDomainInfo {
    pub domain: String,
    pub namespace_id: String,
    pub audience_name: String,
    pub created_at: i64,
    pub verified: bool,
}

/// Repository for namespace-related operations.
pub struct NamespaceRepo {
    conn: Arc<Mutex<Connection>>,
}

impl NamespaceRepo {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    // -------------------------------------------------------------------------
    // Namespaces
    // -------------------------------------------------------------------------

    pub fn create_namespace(&self, namespace_id: &str, owner_user_id: &str) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();
        conn.execute(
            "INSERT INTO namespaces (id, owner_user_id, created_at) VALUES (?1, ?2, ?3)",
            params![namespace_id, owner_user_id, now],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    pub fn get_namespace(&self, namespace_id: &str) -> Option<NamespaceInfo> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, owner_user_id, created_at FROM namespaces WHERE id = ?1",
            params![namespace_id],
            |row| {
                Ok(NamespaceInfo {
                    id: row.get(0)?,
                    owner_user_id: row.get(1)?,
                    created_at: row.get(2)?,
                })
            },
        )
        .optional()
        .unwrap_or(None)
    }

    pub fn list_namespaces(
        &self,
        owner_user_id: &str,
        limit: u32,
        offset: u32,
    ) -> Vec<NamespaceInfo> {
        let conn = self.conn.lock().unwrap();
        conn.prepare(
            "SELECT id, owner_user_id, created_at FROM namespaces WHERE owner_user_id = ?1 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3",
        )
        .and_then(|mut stmt| {
            stmt.query_map(params![owner_user_id, limit, offset], |row| {
                Ok(NamespaceInfo {
                    id: row.get(0)?,
                    owner_user_id: row.get(1)?,
                    created_at: row.get(2)?,
                })
            })
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
        })
        .unwrap_or_default()
    }

    pub fn delete_namespace(&self, namespace_id: &str) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM namespaces WHERE id = ?1",
            params![namespace_id],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    // -------------------------------------------------------------------------
    // Objects
    // -------------------------------------------------------------------------

    /// Upsert object metadata (R2-backed; inline data stored separately).
    pub fn upsert_object(
        &self,
        namespace_id: &str,
        key: &str,
        r2_key: &str,
        mime_type: &str,
        size_bytes: u64,
        audience: Option<&str>,
    ) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();
        conn.execute(
            "INSERT INTO namespace_objects (namespace_id, key, r2_key, data, mime_type, size_bytes, updated_at, audience)
             VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?6, ?7)
             ON CONFLICT(namespace_id, key) DO UPDATE SET
               r2_key = excluded.r2_key,
               data = NULL,
               mime_type = excluded.mime_type,
               size_bytes = excluded.size_bytes,
               updated_at = excluded.updated_at,
               audience = excluded.audience",
            params![namespace_id, key, r2_key, mime_type, size_bytes as i64, now, audience],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    pub fn get_object_meta(&self, namespace_id: &str, key: &str) -> Option<NamespaceObjectMeta> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT namespace_id, key, r2_key, mime_type, size_bytes, updated_at, audience
             FROM namespace_objects WHERE namespace_id = ?1 AND key = ?2",
            params![namespace_id, key],
            |row| {
                Ok(NamespaceObjectMeta {
                    namespace_id: row.get(0)?,
                    key: row.get(1)?,
                    r2_key: row.get(2)?,
                    mime_type: row.get(3)?,
                    size_bytes: row.get::<_, i64>(4)?.max(0) as u64,
                    updated_at: row.get(5)?,
                    audience: row.get(6)?,
                })
            },
        )
        .optional()
        .unwrap_or(None)
    }

    pub fn list_objects(
        &self,
        namespace_id: &str,
        limit: u32,
        offset: u32,
    ) -> Vec<NamespaceObjectMeta> {
        let conn = self.conn.lock().unwrap();
        conn.prepare(
            "SELECT namespace_id, key, r2_key, mime_type, size_bytes, updated_at, audience
             FROM namespace_objects WHERE namespace_id = ?1 ORDER BY key LIMIT ?2 OFFSET ?3",
        )
        .and_then(|mut stmt| {
            stmt.query_map(params![namespace_id, limit, offset], |row| {
                Ok(NamespaceObjectMeta {
                    namespace_id: row.get(0)?,
                    key: row.get(1)?,
                    r2_key: row.get(2)?,
                    mime_type: row.get(3)?,
                    size_bytes: row.get::<_, i64>(4)?.max(0) as u64,
                    updated_at: row.get(5)?,
                    audience: row.get(6)?,
                })
            })
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
        })
        .unwrap_or_default()
    }

    pub fn delete_object(&self, namespace_id: &str, key: &str) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM namespace_objects WHERE namespace_id = ?1 AND key = ?2",
            params![namespace_id, key],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    /// List objects belonging to a specific audience.
    pub fn list_objects_by_audience(
        &self,
        namespace_id: &str,
        audience: &str,
        limit: u32,
        offset: u32,
    ) -> Vec<NamespaceObjectMeta> {
        let conn = self.conn.lock().unwrap();
        conn.prepare(
            "SELECT namespace_id, key, r2_key, mime_type, size_bytes, updated_at, audience
             FROM namespace_objects WHERE namespace_id = ?1 AND audience = ?2 ORDER BY key LIMIT ?3 OFFSET ?4",
        )
        .and_then(|mut stmt| {
            stmt.query_map(params![namespace_id, audience, limit, offset], |row| {
                Ok(NamespaceObjectMeta {
                    namespace_id: row.get(0)?,
                    key: row.get(1)?,
                    r2_key: row.get(2)?,
                    mime_type: row.get(3)?,
                    size_bytes: row.get::<_, i64>(4)?.max(0) as u64,
                    updated_at: row.get(5)?,
                    audience: row.get(6)?,
                })
            })
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
        })
        .unwrap_or_default()
    }

    /// NULL out the audience field on all objects referencing a deleted audience.
    pub fn clear_objects_audience(
        &self,
        namespace_id: &str,
        audience_name: &str,
    ) -> Result<u64, String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE namespace_objects SET audience = NULL WHERE namespace_id = ?1 AND audience = ?2",
            params![namespace_id, audience_name],
        )
        .map(|changed| changed as u64)
        .map_err(|e| e.to_string())
    }

    // -------------------------------------------------------------------------
    // Custom domains
    // -------------------------------------------------------------------------

    pub fn upsert_custom_domain(
        &self,
        domain: &str,
        namespace_id: &str,
        audience_name: &str,
    ) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();
        conn.execute(
            "INSERT INTO custom_domains (domain, namespace_id, audience_name, created_at, verified)
             VALUES (?1, ?2, ?3, ?4, 0)
             ON CONFLICT(domain) DO UPDATE SET
               namespace_id = excluded.namespace_id,
               audience_name = excluded.audience_name",
            params![domain, namespace_id, audience_name, now],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    pub fn get_custom_domain(&self, domain: &str) -> Option<CustomDomainInfo> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT domain, namespace_id, audience_name, created_at, verified
             FROM custom_domains WHERE domain = ?1",
            params![domain],
            |row| {
                Ok(CustomDomainInfo {
                    domain: row.get(0)?,
                    namespace_id: row.get(1)?,
                    audience_name: row.get(2)?,
                    created_at: row.get(3)?,
                    verified: row.get(4)?,
                })
            },
        )
        .optional()
        .unwrap_or(None)
    }

    pub fn list_custom_domains(&self, namespace_id: &str) -> Vec<CustomDomainInfo> {
        let conn = self.conn.lock().unwrap();
        conn.prepare(
            "SELECT domain, namespace_id, audience_name, created_at, verified
             FROM custom_domains WHERE namespace_id = ?1 ORDER BY domain",
        )
        .and_then(|mut stmt| {
            stmt.query_map(params![namespace_id], |row| {
                Ok(CustomDomainInfo {
                    domain: row.get(0)?,
                    namespace_id: row.get(1)?,
                    audience_name: row.get(2)?,
                    created_at: row.get(3)?,
                    verified: row.get(4)?,
                })
            })
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
        })
        .unwrap_or_default()
    }

    pub fn delete_custom_domain(&self, domain: &str) -> Result<bool, String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM custom_domains WHERE domain = ?1",
            params![domain],
        )
        .map(|changed| changed > 0)
        .map_err(|e| e.to_string())
    }

    // -------------------------------------------------------------------------
    // Audiences
    // -------------------------------------------------------------------------

    pub fn upsert_audience(
        &self,
        namespace_id: &str,
        audience_name: &str,
        access: &str,
    ) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO namespace_audiences (namespace_id, audience_name, access)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(namespace_id, audience_name) DO UPDATE SET access = excluded.access",
            params![namespace_id, audience_name, access],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    pub fn get_audience(&self, namespace_id: &str, audience_name: &str) -> Option<AudienceInfo> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT namespace_id, audience_name, access FROM namespace_audiences
             WHERE namespace_id = ?1 AND audience_name = ?2",
            params![namespace_id, audience_name],
            |row| {
                Ok(AudienceInfo {
                    namespace_id: row.get(0)?,
                    audience_name: row.get(1)?,
                    access: row.get(2)?,
                })
            },
        )
        .optional()
        .unwrap_or(None)
    }

    pub fn list_audiences(&self, namespace_id: &str) -> Vec<AudienceInfo> {
        let conn = self.conn.lock().unwrap();
        conn.prepare(
            "SELECT namespace_id, audience_name, access FROM namespace_audiences
             WHERE namespace_id = ?1 ORDER BY audience_name",
        )
        .and_then(|mut stmt| {
            stmt.query_map(params![namespace_id], |row| {
                Ok(AudienceInfo {
                    namespace_id: row.get(0)?,
                    audience_name: row.get(1)?,
                    access: row.get(2)?,
                })
            })
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
        })
        .unwrap_or_default()
    }

    pub fn delete_audience(&self, namespace_id: &str, audience_name: &str) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM namespace_audiences WHERE namespace_id = ?1 AND audience_name = ?2",
            params![namespace_id, audience_name],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    // -------------------------------------------------------------------------
    // Usage metering
    // -------------------------------------------------------------------------

    pub fn record_usage(
        &self,
        user_id: &str,
        event_type: &str,
        amount: u64,
        namespace_id: Option<&str>,
    ) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();
        conn.execute(
            "INSERT INTO usage_events (user_id, event_type, amount, namespace_id, recorded_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![user_id, event_type, amount as i64, namespace_id, now],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    // -------------------------------------------------------------------------
    // Namespace sessions
    // -------------------------------------------------------------------------

    /// Create a namespace session, returning the generated code.
    pub fn create_session(
        &self,
        namespace_id: &str,
        owner_user_id: &str,
        read_only: bool,
        expires_at: Option<i64>,
    ) -> Result<String, String> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();
        let code = generate_session_code();
        conn.execute(
            "INSERT INTO namespace_sessions (code, namespace_id, owner_user_id, read_only, created_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![code, namespace_id, owner_user_id, read_only, now, expires_at],
        )
        .map(|_| code)
        .map_err(|e| e.to_string())
    }

    /// Get a namespace session by code (excludes expired).
    pub fn get_session(&self, code: &str) -> Option<NamespaceSessionInfo> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();
        conn.query_row(
            "SELECT code, namespace_id, owner_user_id, read_only, created_at, expires_at
             FROM namespace_sessions
             WHERE code = ?1 AND (expires_at IS NULL OR expires_at > ?2)",
            params![code, now],
            |row| {
                Ok(NamespaceSessionInfo {
                    code: row.get(0)?,
                    namespace_id: row.get(1)?,
                    owner_user_id: row.get(2)?,
                    read_only: row.get(3)?,
                    created_at: row.get(4)?,
                    expires_at: row.get(5)?,
                })
            },
        )
        .optional()
        .unwrap_or(None)
    }

    /// List non-expired sessions owned by a user.
    pub fn list_sessions(&self, owner_user_id: &str) -> Vec<NamespaceSessionInfo> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();
        conn.prepare(
            "SELECT code, namespace_id, owner_user_id, read_only, created_at, expires_at
             FROM namespace_sessions
             WHERE owner_user_id = ?1 AND (expires_at IS NULL OR expires_at > ?2)
             ORDER BY created_at DESC",
        )
        .and_then(|mut stmt| {
            stmt.query_map(params![owner_user_id, now], |row| {
                Ok(NamespaceSessionInfo {
                    code: row.get(0)?,
                    namespace_id: row.get(1)?,
                    owner_user_id: row.get(2)?,
                    read_only: row.get(3)?,
                    created_at: row.get(4)?,
                    expires_at: row.get(5)?,
                })
            })
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
        })
        .unwrap_or_default()
    }

    /// Update the read_only flag on a session. Returns true if a row was updated.
    pub fn update_session_read_only(&self, code: &str, read_only: bool) -> Result<bool, String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE namespace_sessions SET read_only = ?1 WHERE code = ?2",
            params![read_only, code],
        )
        .map(|changed| changed > 0)
        .map_err(|e| e.to_string())
    }

    /// Delete a session by code. Returns true if a row was deleted.
    pub fn delete_session(&self, code: &str) -> Result<bool, String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM namespace_sessions WHERE code = ?1",
            params![code],
        )
        .map(|changed| changed > 0)
        .map_err(|e| e.to_string())
    }

    // -------------------------------------------------------------------------
    // Usage metering
    // -------------------------------------------------------------------------

    /// Aggregate usage totals for a user over all time.
    pub fn get_usage_totals(&self, user_id: &str) -> UsageTotals {
        let conn = self.conn.lock().unwrap();
        let sum_for = |event_type: &str| -> u64 {
            conn.query_row(
                "SELECT COALESCE(SUM(amount), 0) FROM usage_events WHERE user_id = ?1 AND event_type = ?2",
                params![user_id, event_type],
                |row| row.get::<_, i64>(0),
            )
            .map(|v| v.max(0) as u64)
            .unwrap_or(0)
        };
        UsageTotals {
            bytes_in: sum_for("bytes_in"),
            bytes_out: sum_for("bytes_out"),
            relay_seconds: sum_for("relay_seconds"),
        }
    }

    /// Aggregate usage totals for a user scoped to a specific namespace.
    pub fn get_namespace_usage_totals(&self, user_id: &str, namespace_id: &str) -> UsageTotals {
        let conn = self.conn.lock().unwrap();
        let sum_for = |event_type: &str| -> u64 {
            conn.query_row(
                "SELECT COALESCE(SUM(amount), 0) FROM usage_events WHERE user_id = ?1 AND event_type = ?2 AND namespace_id = ?3",
                params![user_id, event_type, namespace_id],
                |row| row.get::<_, i64>(0),
            )
            .map(|v| v.max(0) as u64)
            .unwrap_or(0)
        };
        UsageTotals {
            bytes_in: sum_for("bytes_in"),
            bytes_out: sum_for("bytes_out"),
            relay_seconds: sum_for("relay_seconds"),
        }
    }
}

/// Generate a session code in XXXXXXXX-XXXXXXXX format.
fn generate_session_code() -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();
    let part = |rng: &mut rand::rngs::ThreadRng| -> String {
        (0..8)
            .map(|_| {
                let idx = rng.gen_range(0..CHARSET.len());
                CHARSET[idx] as char
            })
            .collect()
    };
    format!("{}-{}", part(&mut rng), part(&mut rng))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema::init_database;
    use rusqlite::Connection;

    fn make_repo_with_schema() -> NamespaceRepo {
        let conn = Connection::open_in_memory().unwrap();
        init_database(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO users (id, email, created_at, tier) VALUES ('u1', 'u1@test.com', 0, 'free')",
        ).unwrap();
        let conn = Arc::new(Mutex::new(conn));
        NamespaceRepo::new(conn)
    }

    #[test]
    fn namespace_crud() {
        let repo = make_repo_with_schema();

        repo.create_namespace("workspace:abc", "u1").unwrap();

        let ns = repo.get_namespace("workspace:abc").unwrap();
        assert_eq!(ns.id, "workspace:abc");
        assert_eq!(ns.owner_user_id, "u1");

        let list = repo.list_namespaces("u1", 100, 0);
        assert_eq!(list.len(), 1);

        repo.delete_namespace("workspace:abc").unwrap();
        assert!(repo.get_namespace("workspace:abc").is_none());
    }

    #[test]
    fn object_crud() {
        let repo = make_repo_with_schema();
        repo.create_namespace("workspace:abc", "u1").unwrap();
        repo.upsert_object(
            "workspace:abc",
            "README.md",
            "ns/workspace:abc/README.md",
            "text/markdown",
            42,
            None,
        )
        .unwrap();

        let meta = repo.get_object_meta("workspace:abc", "README.md").unwrap();
        assert_eq!(meta.size_bytes, 42);
        assert_eq!(meta.r2_key.as_deref(), Some("ns/workspace:abc/README.md"));
        assert!(meta.audience.is_none());

        // Upsert updates existing
        repo.upsert_object(
            "workspace:abc",
            "README.md",
            "ns/workspace:abc/README.md",
            "text/markdown",
            100,
            Some("public"),
        )
        .unwrap();
        let meta2 = repo.get_object_meta("workspace:abc", "README.md").unwrap();
        assert_eq!(meta2.size_bytes, 100);
        assert_eq!(meta2.audience.as_deref(), Some("public"));

        let list = repo.list_objects("workspace:abc", 100, 0);
        assert_eq!(list.len(), 1);

        repo.delete_object("workspace:abc", "README.md").unwrap();
        assert!(repo.get_object_meta("workspace:abc", "README.md").is_none());
    }

    #[test]
    fn object_audience_round_trip() {
        let repo = make_repo_with_schema();
        repo.create_namespace("site:x", "u1").unwrap();

        // Private (no audience)
        repo.upsert_object("site:x", "a.txt", "ns/site:x/a.txt", "text/plain", 10, None)
            .unwrap();
        // With audience
        repo.upsert_object(
            "site:x",
            "b.txt",
            "ns/site:x/b.txt",
            "text/plain",
            20,
            Some("public"),
        )
        .unwrap();
        repo.upsert_object(
            "site:x",
            "c.txt",
            "ns/site:x/c.txt",
            "text/plain",
            30,
            Some("public"),
        )
        .unwrap();
        repo.upsert_object(
            "site:x",
            "d.txt",
            "ns/site:x/d.txt",
            "text/plain",
            40,
            Some("members"),
        )
        .unwrap();

        let by_public = repo.list_objects_by_audience("site:x", "public", 100, 0);
        assert_eq!(by_public.len(), 2);

        let by_members = repo.list_objects_by_audience("site:x", "members", 100, 0);
        assert_eq!(by_members.len(), 1);

        // clear_objects_audience
        let cleared = repo.clear_objects_audience("site:x", "public").unwrap();
        assert_eq!(cleared, 2);
        let after = repo.list_objects_by_audience("site:x", "public", 100, 0);
        assert!(after.is_empty());
        // Objects still exist, just audience is NULL
        let b = repo.get_object_meta("site:x", "b.txt").unwrap();
        assert!(b.audience.is_none());
    }

    #[test]
    fn custom_domain_crud() {
        let repo = make_repo_with_schema();
        repo.create_namespace("site:x", "u1").unwrap();

        repo.upsert_custom_domain("blog.example.com", "site:x", "public")
            .unwrap();
        let d = repo.get_custom_domain("blog.example.com").unwrap();
        assert_eq!(d.namespace_id, "site:x");
        assert_eq!(d.audience_name, "public");
        assert!(!d.verified);

        let list = repo.list_custom_domains("site:x");
        assert_eq!(list.len(), 1);

        // Update audience
        repo.upsert_custom_domain("blog.example.com", "site:x", "members")
            .unwrap();
        let d2 = repo.get_custom_domain("blog.example.com").unwrap();
        assert_eq!(d2.audience_name, "members");

        assert!(repo.delete_custom_domain("blog.example.com").unwrap());
        assert!(repo.get_custom_domain("blog.example.com").is_none());
    }

    #[test]
    fn audience_crud() {
        let repo = make_repo_with_schema();
        repo.create_namespace("site:abc", "u1").unwrap();

        repo.upsert_audience("site:abc", "public", "public")
            .unwrap();
        repo.upsert_audience("site:abc", "members", "token")
            .unwrap();

        let list = repo.list_audiences("site:abc");
        assert_eq!(list.len(), 2);

        let a = repo.get_audience("site:abc", "public").unwrap();
        assert_eq!(a.access, "public");

        // Update
        repo.upsert_audience("site:abc", "public", "private")
            .unwrap();
        let a2 = repo.get_audience("site:abc", "public").unwrap();
        assert_eq!(a2.access, "private");

        repo.delete_audience("site:abc", "public").unwrap();
        assert!(repo.get_audience("site:abc", "public").is_none());
    }

    #[test]
    fn session_crud() {
        let repo = make_repo_with_schema();
        repo.create_namespace("workspace:abc", "u1").unwrap();

        let code = repo
            .create_session("workspace:abc", "u1", false, None)
            .unwrap();
        assert_eq!(code.len(), 17); // XXXXXXXX-XXXXXXXX

        let session = repo.get_session(&code).unwrap();
        assert_eq!(session.namespace_id, "workspace:abc");
        assert!(!session.read_only);

        // Update read_only
        assert!(repo.update_session_read_only(&code, true).unwrap());
        let session2 = repo.get_session(&code).unwrap();
        assert!(session2.read_only);

        // List
        let list = repo.list_sessions("u1");
        assert_eq!(list.len(), 1);

        // Delete
        assert!(repo.delete_session(&code).unwrap());
        assert!(repo.get_session(&code).is_none());
    }

    #[test]
    fn usage_recording_and_totals() {
        let repo = make_repo_with_schema();
        repo.record_usage("u1", "bytes_in", 1000, Some("workspace:abc"))
            .unwrap();
        repo.record_usage("u1", "bytes_in", 500, None).unwrap();
        repo.record_usage("u1", "bytes_out", 200, None).unwrap();
        repo.record_usage("u1", "relay_seconds", 30, None).unwrap();

        let totals = repo.get_usage_totals("u1");
        assert_eq!(totals.bytes_in, 1500);
        assert_eq!(totals.bytes_out, 200);
        assert_eq!(totals.relay_seconds, 30);

        // Namespace-scoped totals
        let ns_totals = repo.get_namespace_usage_totals("u1", "workspace:abc");
        assert_eq!(ns_totals.bytes_in, 1000);
        assert_eq!(ns_totals.bytes_out, 0);
        assert_eq!(ns_totals.relay_seconds, 0);
    }
}
