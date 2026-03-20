//! D1 (SQLite) adapter implementations for Cloudflare Workers.
//!
//! D1 uses the same SQL as the native SQLite adapter — only the async
//! binding API differs.
//!
//! NOTE: D1 types are `!Send`. The `cfg_async_trait!` macro on
//! `diaryx_server` traits emits `#[async_trait(?Send)]` on wasm32,
//! so the futures don't require `Send`. However, the traits still
//! declare `Send + Sync` supertraits. This compiles because on
//! wasm32-unknown-unknown there is no threading — all types are
//! trivially `Send + Sync`. If the `worker` crate opts out of
//! auto-`Send` via negative impls, wrapper types may be needed.

use async_trait::async_trait;
use diaryx_server::domain::*;
use diaryx_server::ports::*;
use worker::D1Database;

fn e(err: impl std::fmt::Display) -> ServerCoreError {
    ServerCoreError::internal(err.to_string())
}

/// Convert an i64 timestamp to a D1-compatible JsValue.
/// D1 does not support bigint; cast to f64 (safe for Unix timestamps).
fn ts(epoch: i64) -> worker::wasm_bindgen::JsValue {
    worker::wasm_bindgen::JsValue::from_f64(epoch as f64)
}

// ---------------------------------------------------------------------------
// NamespaceStore
// ---------------------------------------------------------------------------

pub struct D1NamespaceStore {
    db: D1Database,
}

impl D1NamespaceStore {
    pub fn new(db: D1Database) -> Self {
        Self { db }
    }
}

#[async_trait(?Send)]
impl NamespaceStore for D1NamespaceStore {
    async fn get_namespace(
        &self,
        namespace_id: &str,
    ) -> Result<Option<NamespaceInfo>, ServerCoreError> {
        let stmt = self
            .db
            .prepare("SELECT id, owner_user_id, created_at FROM namespaces WHERE id = ?1")
            .bind(&[namespace_id.into()])
            .map_err(e)?;

        let result = stmt.first::<serde_json::Value>(None).await.map_err(e)?;

        Ok(result.map(|row| NamespaceInfo {
            id: row["id"].as_str().unwrap_or_default().to_string(),
            owner_user_id: row["owner_user_id"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            created_at: row["created_at"].as_i64().unwrap_or_default(),
        }))
    }

    async fn list_namespaces(
        &self,
        owner_user_id: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<NamespaceInfo>, ServerCoreError> {
        let stmt = self
            .db
            .prepare(
                "SELECT id, owner_user_id, created_at FROM namespaces \
                 WHERE owner_user_id = ?1 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3",
            )
            .bind(&[owner_user_id.into(), limit.into(), offset.into()])
            .map_err(e)?;

        let results = stmt.all().await.map_err(e)?;
        let rows: Vec<serde_json::Value> = results.results().map_err(e)?;

        Ok(rows
            .into_iter()
            .map(|row| NamespaceInfo {
                id: row["id"].as_str().unwrap_or_default().to_string(),
                owner_user_id: row["owner_user_id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                created_at: row["created_at"].as_i64().unwrap_or_default(),
            })
            .collect())
    }

    async fn create_namespace(
        &self,
        namespace_id: &str,
        owner_user_id: &str,
    ) -> Result<(), ServerCoreError> {
        let now = chrono::Utc::now().timestamp();
        self.db
            .prepare("INSERT INTO namespaces (id, owner_user_id, created_at) VALUES (?1, ?2, ?3)")
            .bind(&[namespace_id.into(), owner_user_id.into(), ts(now)])
            .map_err(e)?
            .run()
            .await
            .map_err(e)?;
        Ok(())
    }

    async fn delete_namespace(&self, namespace_id: &str) -> Result<(), ServerCoreError> {
        self.db
            .prepare("DELETE FROM namespaces WHERE id = ?1")
            .bind(&[namespace_id.into()])
            .map_err(e)?
            .run()
            .await
            .map_err(e)?;
        Ok(())
    }

    async fn get_audience(
        &self,
        namespace_id: &str,
        audience_name: &str,
    ) -> Result<Option<AudienceInfo>, ServerCoreError> {
        let result = self
            .db
            .prepare(
                "SELECT namespace_id, audience_name, access FROM namespace_audiences \
                 WHERE namespace_id = ?1 AND audience_name = ?2",
            )
            .bind(&[namespace_id.into(), audience_name.into()])
            .map_err(e)?
            .first::<serde_json::Value>(None)
            .await
            .map_err(e)?;

        Ok(result.map(|row| AudienceInfo {
            namespace_id: row["namespace_id"].as_str().unwrap_or_default().to_string(),
            audience_name: row["audience_name"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            access: row["access"].as_str().unwrap_or_default().to_string(),
        }))
    }

    async fn upsert_audience(
        &self,
        namespace_id: &str,
        audience_name: &str,
        access: &str,
    ) -> Result<(), ServerCoreError> {
        self.db
            .prepare(
                "INSERT INTO namespace_audiences (namespace_id, audience_name, access) \
                 VALUES (?1, ?2, ?3) \
                 ON CONFLICT(namespace_id, audience_name) DO UPDATE SET access = excluded.access",
            )
            .bind(&[namespace_id.into(), audience_name.into(), access.into()])
            .map_err(e)?
            .run()
            .await
            .map_err(e)?;
        Ok(())
    }

    async fn list_audiences(
        &self,
        namespace_id: &str,
    ) -> Result<Vec<AudienceInfo>, ServerCoreError> {
        let results = self
            .db
            .prepare(
                "SELECT namespace_id, audience_name, access FROM namespace_audiences \
                 WHERE namespace_id = ?1 ORDER BY audience_name",
            )
            .bind(&[namespace_id.into()])
            .map_err(e)?
            .all()
            .await
            .map_err(e)?;

        let rows: Vec<serde_json::Value> = results.results().map_err(e)?;
        Ok(rows
            .into_iter()
            .map(|row| AudienceInfo {
                namespace_id: row["namespace_id"].as_str().unwrap_or_default().to_string(),
                audience_name: row["audience_name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                access: row["access"].as_str().unwrap_or_default().to_string(),
            })
            .collect())
    }

    async fn delete_audience(
        &self,
        namespace_id: &str,
        audience_name: &str,
    ) -> Result<(), ServerCoreError> {
        self.db
            .prepare(
                "DELETE FROM namespace_audiences WHERE namespace_id = ?1 AND audience_name = ?2",
            )
            .bind(&[namespace_id.into(), audience_name.into()])
            .map_err(e)?
            .run()
            .await
            .map_err(e)?;
        Ok(())
    }

    async fn clear_objects_audience(
        &self,
        namespace_id: &str,
        audience_name: &str,
    ) -> Result<(), ServerCoreError> {
        self.db
            .prepare(
                "UPDATE namespace_objects SET audience = NULL \
                 WHERE namespace_id = ?1 AND audience = ?2",
            )
            .bind(&[namespace_id.into(), audience_name.into()])
            .map_err(e)?
            .run()
            .await
            .map_err(e)?;
        Ok(())
    }

    async fn get_custom_domain(
        &self,
        domain: &str,
    ) -> Result<Option<CustomDomainInfo>, ServerCoreError> {
        let result = self
            .db
            .prepare(
                "SELECT domain, namespace_id, audience_name, created_at, verified \
                 FROM custom_domains WHERE domain = ?1",
            )
            .bind(&[domain.into()])
            .map_err(e)?
            .first::<serde_json::Value>(None)
            .await
            .map_err(e)?;

        Ok(result.map(|row| CustomDomainInfo {
            domain: row["domain"].as_str().unwrap_or_default().to_string(),
            namespace_id: row["namespace_id"].as_str().unwrap_or_default().to_string(),
            audience_name: row["audience_name"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            created_at: row["created_at"].as_i64().unwrap_or_default(),
            verified: row["verified"].as_bool().unwrap_or_default(),
        }))
    }

    async fn list_custom_domains(
        &self,
        namespace_id: &str,
    ) -> Result<Vec<CustomDomainInfo>, ServerCoreError> {
        let results = self
            .db
            .prepare(
                "SELECT domain, namespace_id, audience_name, created_at, verified \
                 FROM custom_domains WHERE namespace_id = ?1 ORDER BY domain",
            )
            .bind(&[namespace_id.into()])
            .map_err(e)?
            .all()
            .await
            .map_err(e)?;

        let rows: Vec<serde_json::Value> = results.results().map_err(e)?;
        Ok(rows
            .into_iter()
            .map(|row| CustomDomainInfo {
                domain: row["domain"].as_str().unwrap_or_default().to_string(),
                namespace_id: row["namespace_id"].as_str().unwrap_or_default().to_string(),
                audience_name: row["audience_name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                created_at: row["created_at"].as_i64().unwrap_or_default(),
                verified: row["verified"].as_bool().unwrap_or_default(),
            })
            .collect())
    }

    async fn upsert_custom_domain(
        &self,
        domain: &str,
        namespace_id: &str,
        audience_name: &str,
    ) -> Result<(), ServerCoreError> {
        let now = chrono::Utc::now().timestamp();
        self.db
            .prepare(
                "INSERT INTO custom_domains (domain, namespace_id, audience_name, created_at, verified) \
                 VALUES (?1, ?2, ?3, ?4, 0) \
                 ON CONFLICT(domain) DO UPDATE SET \
                   namespace_id = excluded.namespace_id, \
                   audience_name = excluded.audience_name",
            )
            .bind(&[domain.into(), namespace_id.into(), audience_name.into(), ts(now)])
            .map_err(e)?
            .run()
            .await
            .map_err(e)?;
        Ok(())
    }

    async fn delete_custom_domain(&self, domain: &str) -> Result<bool, ServerCoreError> {
        let meta = self
            .db
            .prepare("DELETE FROM custom_domains WHERE domain = ?1")
            .bind(&[domain.into()])
            .map_err(e)?
            .run()
            .await
            .map_err(e)?;
        Ok(meta.success())
    }
}

// ---------------------------------------------------------------------------
// AuthSessionStore
// ---------------------------------------------------------------------------

pub struct D1AuthSessionStore {
    db: D1Database,
}

impl D1AuthSessionStore {
    pub fn new(db: D1Database) -> Self {
        Self { db }
    }
}

#[async_trait(?Send)]
impl AuthSessionStore for D1AuthSessionStore {
    async fn validate_session(
        &self,
        token: &str,
    ) -> Result<Option<AuthSessionInfo>, ServerCoreError> {
        let now = chrono::Utc::now().timestamp();
        let result = self
            .db
            .prepare(
                "SELECT token, user_id, device_id, expires_at, created_at \
                 FROM auth_sessions WHERE token = ?1 AND expires_at > ?2",
            )
            .bind(&[token.into(), ts(now)])
            .map_err(e)?
            .first::<serde_json::Value>(None)
            .await
            .map_err(e)?;

        Ok(result.map(|row| {
            let expires_ts = row["expires_at"].as_i64().unwrap_or_default();
            let created_ts = row["created_at"].as_i64().unwrap_or_default();
            AuthSessionInfo {
                token: row["token"].as_str().unwrap_or_default().to_string(),
                user_id: row["user_id"].as_str().unwrap_or_default().to_string(),
                device_id: row["device_id"].as_str().unwrap_or_default().to_string(),
                expires_at: chrono::DateTime::from_timestamp(expires_ts, 0)
                    .unwrap_or_else(chrono::Utc::now),
                created_at: chrono::DateTime::from_timestamp(created_ts, 0)
                    .unwrap_or_else(chrono::Utc::now),
            }
        }))
    }

    async fn create_auth_session(
        &self,
        user_id: &str,
        device_id: &str,
        expires_at_unix: i64,
    ) -> Result<String, ServerCoreError> {
        let token = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp();
        self.db
            .prepare(
                "INSERT INTO auth_sessions (token, user_id, device_id, expires_at, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )
            .bind(&[
                token.as_str().into(),
                user_id.into(),
                device_id.into(),
                ts(expires_at_unix),
                ts(now),
            ])
            .map_err(e)?
            .run()
            .await
            .map_err(e)?;
        Ok(token)
    }

    async fn delete_session(&self, token: &str) -> Result<(), ServerCoreError> {
        self.db
            .prepare("DELETE FROM auth_sessions WHERE token = ?1")
            .bind(&[token.into()])
            .map_err(e)?
            .run()
            .await
            .map_err(e)?;
        Ok(())
    }

    async fn update_device_last_seen(&self, device_id: &str) -> Result<(), ServerCoreError> {
        let now = chrono::Utc::now().timestamp();
        self.db
            .prepare("UPDATE devices SET last_seen_at = ?1 WHERE id = ?2")
            .bind(&[ts(now), device_id.into()])
            .map_err(e)?
            .run()
            .await
            .map_err(e)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// AuthStore (read-only user/device queries for /auth/me and middleware)
// ---------------------------------------------------------------------------

pub struct D1AuthStore {
    db: D1Database,
}

impl D1AuthStore {
    pub fn new(db: D1Database) -> Self {
        Self { db }
    }
}

#[async_trait(?Send)]
impl AuthStore for D1AuthStore {
    async fn get_user(&self, user_id: &str) -> Result<Option<UserInfo>, ServerCoreError> {
        let result = self
            .db
            .prepare(
                "SELECT id, email, created_at, last_login_at, attachment_limit_bytes, \
                 workspace_limit, tier, published_site_limit FROM users WHERE id = ?1",
            )
            .bind(&[user_id.into()])
            .map_err(e)?
            .first::<serde_json::Value>(None)
            .await
            .map_err(e)?;

        Ok(result.map(|row| UserInfo {
            id: row["id"].as_str().unwrap_or_default().to_string(),
            email: row["email"].as_str().unwrap_or_default().to_string(),
            created_at: chrono::DateTime::from_timestamp(
                row["created_at"].as_i64().unwrap_or_default(),
                0,
            )
            .unwrap_or_else(chrono::Utc::now),
            last_login_at: row["last_login_at"]
                .as_i64()
                .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0)),
            attachment_limit_bytes: row["attachment_limit_bytes"].as_u64(),
            workspace_limit: row["workspace_limit"].as_u64().map(|v| v as u32),
            tier: UserTier::from_str_lossy(row["tier"].as_str().unwrap_or("free")),
            published_site_limit: row["published_site_limit"].as_u64().map(|v| v as u32),
        }))
    }

    async fn list_user_devices(&self, user_id: &str) -> Result<Vec<DeviceInfo>, ServerCoreError> {
        let results = self
            .db
            .prepare(
                "SELECT id, user_id, name, user_agent, created_at, last_seen_at \
                 FROM devices WHERE user_id = ?1 ORDER BY last_seen_at DESC",
            )
            .bind(&[user_id.into()])
            .map_err(e)?
            .all()
            .await
            .map_err(e)?;

        let rows: Vec<serde_json::Value> = results.results().map_err(e)?;
        Ok(rows
            .into_iter()
            .map(|row| DeviceInfo {
                id: row["id"].as_str().unwrap_or_default().to_string(),
                user_id: row["user_id"].as_str().unwrap_or_default().to_string(),
                name: row["name"].as_str().map(|s| s.to_string()),
                user_agent: row["user_agent"].as_str().map(|s| s.to_string()),
                created_at: chrono::DateTime::from_timestamp(
                    row["created_at"].as_i64().unwrap_or_default(),
                    0,
                )
                .unwrap_or_else(chrono::Utc::now),
                last_seen_at: chrono::DateTime::from_timestamp(
                    row["last_seen_at"].as_i64().unwrap_or_default(),
                    0,
                )
                .unwrap_or_else(chrono::Utc::now),
            })
            .collect())
    }

    async fn rename_device(
        &self,
        device_id: &str,
        new_name: &str,
    ) -> Result<bool, ServerCoreError> {
        self.db
            .prepare("UPDATE devices SET name = ?1 WHERE id = ?2")
            .bind(&[new_name.into(), device_id.into()])
            .map_err(e)?
            .run()
            .await
            .map_err(e)?;
        Ok(true)
    }

    async fn delete_device(&self, device_id: &str) -> Result<(), ServerCoreError> {
        self.db
            .prepare("DELETE FROM devices WHERE id = ?1")
            .bind(&[device_id.into()])
            .map_err(e)?
            .run()
            .await
            .map_err(e)?;
        Ok(())
    }

    async fn get_user_tier(&self, user_id: &str) -> Result<UserTier, ServerCoreError> {
        let result = self
            .db
            .prepare("SELECT tier FROM users WHERE id = ?1")
            .bind(&[user_id.into()])
            .map_err(e)?
            .first::<serde_json::Value>(None)
            .await
            .map_err(e)?;

        Ok(result
            .map(|row| UserTier::from_str_lossy(row["tier"].as_str().unwrap_or("free")))
            .unwrap_or(UserTier::Free))
    }
}

// ---------------------------------------------------------------------------
// ObjectMetaStore
// ---------------------------------------------------------------------------

pub struct D1ObjectMetaStore {
    db: D1Database,
}

impl D1ObjectMetaStore {
    pub fn new(db: D1Database) -> Self {
        Self { db }
    }
}

#[async_trait(?Send)]
impl ObjectMetaStore for D1ObjectMetaStore {
    async fn upsert_object(
        &self,
        namespace_id: &str,
        key: &str,
        blob_key: &str,
        mime_type: &str,
        size_bytes: u64,
        audience: Option<&str>,
    ) -> Result<(), ServerCoreError> {
        let now = chrono::Utc::now().timestamp();
        self.db
            .prepare(
                "INSERT INTO namespace_objects (namespace_id, key, r2_key, data, mime_type, size_bytes, updated_at, audience) \
                 VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?6, ?7) \
                 ON CONFLICT(namespace_id, key) DO UPDATE SET \
                   r2_key = excluded.r2_key, data = NULL, mime_type = excluded.mime_type, \
                   size_bytes = excluded.size_bytes, updated_at = excluded.updated_at, \
                   audience = excluded.audience",
            )
            .bind(&[
                namespace_id.into(), key.into(), blob_key.into(),
                mime_type.into(), ts(size_bytes as i64), ts(now),
                audience.unwrap_or("").into(),
            ])
            .map_err(e)?
            .run()
            .await
            .map_err(e)?;
        Ok(())
    }

    async fn get_object_meta(
        &self,
        namespace_id: &str,
        key: &str,
    ) -> Result<Option<ObjectMeta>, ServerCoreError> {
        let result = self
            .db
            .prepare(
                "SELECT namespace_id, key, r2_key, mime_type, size_bytes, updated_at, audience \
                 FROM namespace_objects WHERE namespace_id = ?1 AND key = ?2",
            )
            .bind(&[namespace_id.into(), key.into()])
            .map_err(e)?
            .first::<serde_json::Value>(None)
            .await
            .map_err(e)?;

        Ok(result.map(|row| ObjectMeta {
            namespace_id: row["namespace_id"].as_str().unwrap_or_default().to_string(),
            key: row["key"].as_str().unwrap_or_default().to_string(),
            blob_key: row["r2_key"].as_str().map(|s| s.to_string()),
            mime_type: row["mime_type"].as_str().unwrap_or_default().to_string(),
            size_bytes: row["size_bytes"].as_u64().unwrap_or_default(),
            updated_at: row["updated_at"].as_i64().unwrap_or_default(),
            audience: row["audience"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string()),
        }))
    }

    async fn list_objects(
        &self,
        namespace_id: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<ObjectMeta>, ServerCoreError> {
        let results = self
            .db
            .prepare(
                "SELECT namespace_id, key, r2_key, mime_type, size_bytes, updated_at, audience \
                 FROM namespace_objects WHERE namespace_id = ?1 ORDER BY key LIMIT ?2 OFFSET ?3",
            )
            .bind(&[namespace_id.into(), limit.into(), offset.into()])
            .map_err(e)?
            .all()
            .await
            .map_err(e)?;

        let rows: Vec<serde_json::Value> = results.results().map_err(e)?;
        Ok(rows
            .into_iter()
            .map(|row| ObjectMeta {
                namespace_id: row["namespace_id"].as_str().unwrap_or_default().to_string(),
                key: row["key"].as_str().unwrap_or_default().to_string(),
                blob_key: row["r2_key"].as_str().map(|s| s.to_string()),
                mime_type: row["mime_type"].as_str().unwrap_or_default().to_string(),
                size_bytes: row["size_bytes"].as_u64().unwrap_or_default(),
                updated_at: row["updated_at"].as_i64().unwrap_or_default(),
                audience: row["audience"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string()),
            })
            .collect())
    }

    async fn delete_object(&self, namespace_id: &str, key: &str) -> Result<(), ServerCoreError> {
        self.db
            .prepare("DELETE FROM namespace_objects WHERE namespace_id = ?1 AND key = ?2")
            .bind(&[namespace_id.into(), key.into()])
            .map_err(e)?
            .run()
            .await
            .map_err(e)?;
        Ok(())
    }

    async fn record_usage(
        &self,
        user_id: &str,
        event_type: &str,
        amount: u64,
        namespace_id: Option<&str>,
    ) -> Result<(), ServerCoreError> {
        let now = chrono::Utc::now().timestamp();
        self.db
            .prepare(
                "INSERT INTO usage_events (user_id, event_type, amount, namespace_id, recorded_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )
            .bind(&[
                user_id.into(),
                event_type.into(),
                ts(amount as i64),
                namespace_id.unwrap_or("").into(),
                ts(now),
            ])
            .map_err(e)?
            .run()
            .await
            .map_err(e)?;
        Ok(())
    }

    async fn get_usage_totals(&self, user_id: &str) -> Result<UsageTotals, ServerCoreError> {
        let q = "SELECT COALESCE(SUM(amount), 0) as total FROM usage_events \
                 WHERE user_id = ?1 AND event_type = ?2";
        let get = |r: Option<serde_json::Value>| r.and_then(|v| v["total"].as_u64()).unwrap_or(0);

        let bytes_in = get(self
            .db
            .prepare(q)
            .bind(&[user_id.into(), "bytes_in".into()])
            .map_err(e)?
            .first::<serde_json::Value>(None)
            .await
            .map_err(e)?);
        let bytes_out = get(self
            .db
            .prepare(q)
            .bind(&[user_id.into(), "bytes_out".into()])
            .map_err(e)?
            .first::<serde_json::Value>(None)
            .await
            .map_err(e)?);
        let relay_seconds = get(self
            .db
            .prepare(q)
            .bind(&[user_id.into(), "relay_seconds".into()])
            .map_err(e)?
            .first::<serde_json::Value>(None)
            .await
            .map_err(e)?);

        Ok(UsageTotals {
            bytes_in,
            bytes_out,
            relay_seconds,
        })
    }

    async fn get_namespace_usage_totals(
        &self,
        user_id: &str,
        namespace_id: &str,
    ) -> Result<UsageTotals, ServerCoreError> {
        let q = "SELECT COALESCE(SUM(amount), 0) as total FROM usage_events \
                 WHERE user_id = ?1 AND event_type = ?2 AND namespace_id = ?3";
        let get = |r: Option<serde_json::Value>| r.and_then(|v| v["total"].as_u64()).unwrap_or(0);

        let bytes_in = get(self
            .db
            .prepare(q)
            .bind(&[user_id.into(), "bytes_in".into(), namespace_id.into()])
            .map_err(e)?
            .first::<serde_json::Value>(None)
            .await
            .map_err(e)?);
        let bytes_out = get(self
            .db
            .prepare(q)
            .bind(&[user_id.into(), "bytes_out".into(), namespace_id.into()])
            .map_err(e)?
            .first::<serde_json::Value>(None)
            .await
            .map_err(e)?);
        let relay_seconds = get(self
            .db
            .prepare(q)
            .bind(&[user_id.into(), "relay_seconds".into(), namespace_id.into()])
            .map_err(e)?
            .first::<serde_json::Value>(None)
            .await
            .map_err(e)?);

        Ok(UsageTotals {
            bytes_in,
            bytes_out,
            relay_seconds,
        })
    }
}

// ---------------------------------------------------------------------------
// SessionStore (namespace sessions, not auth sessions)
// ---------------------------------------------------------------------------

pub struct D1SessionStore {
    db: D1Database,
}

impl D1SessionStore {
    pub fn new(db: D1Database) -> Self {
        Self { db }
    }
}

#[async_trait(?Send)]
impl SessionStore for D1SessionStore {
    async fn create_session(
        &self,
        namespace_id: &str,
        owner_user_id: &str,
        read_only: bool,
        expires_at: Option<i64>,
    ) -> Result<String, ServerCoreError> {
        let now = chrono::Utc::now().timestamp();
        let code = generate_session_code();
        self.db
            .prepare(
                "INSERT INTO namespace_sessions (code, namespace_id, owner_user_id, read_only, created_at, expires_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .bind(&[
                code.as_str().into(),
                namespace_id.into(),
                owner_user_id.into(),
                read_only.into(),
                ts(now),
                ts(expires_at.unwrap_or(0)),
            ])
            .map_err(e)?
            .run()
            .await
            .map_err(e)?;
        Ok(code)
    }

    async fn get_session(
        &self,
        code: &str,
    ) -> Result<Option<NamespaceSessionInfo>, ServerCoreError> {
        let now = chrono::Utc::now().timestamp();
        let result = self
            .db
            .prepare(
                "SELECT code, namespace_id, owner_user_id, read_only, created_at, expires_at \
                 FROM namespace_sessions \
                 WHERE code = ?1 AND (expires_at IS NULL OR expires_at = 0 OR expires_at > ?2)",
            )
            .bind(&[code.into(), ts(now)])
            .map_err(e)?
            .first::<serde_json::Value>(None)
            .await
            .map_err(e)?;

        Ok(result.map(|row| NamespaceSessionInfo {
            code: row["code"].as_str().unwrap_or_default().to_string(),
            namespace_id: row["namespace_id"].as_str().unwrap_or_default().to_string(),
            owner_user_id: row["owner_user_id"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            read_only: row["read_only"].as_bool().unwrap_or_default(),
            created_at: row["created_at"].as_i64().unwrap_or_default(),
            expires_at: row["expires_at"].as_i64().filter(|&v| v > 0),
        }))
    }

    async fn update_session_read_only(
        &self,
        code: &str,
        read_only: bool,
    ) -> Result<bool, ServerCoreError> {
        self.db
            .prepare("UPDATE namespace_sessions SET read_only = ?1 WHERE code = ?2")
            .bind(&[read_only.into(), code.into()])
            .map_err(e)?
            .run()
            .await
            .map_err(e)?;
        Ok(true)
    }

    async fn delete_session(&self, code: &str) -> Result<bool, ServerCoreError> {
        self.db
            .prepare("DELETE FROM namespace_sessions WHERE code = ?1")
            .bind(&[code.into()])
            .map_err(e)?
            .run()
            .await
            .map_err(e)?;
        Ok(true)
    }
}

// ---------------------------------------------------------------------------
// MagicLinkStore
// ---------------------------------------------------------------------------

pub struct D1MagicLinkStore {
    db: D1Database,
}

impl D1MagicLinkStore {
    pub fn new(db: D1Database) -> Self {
        Self { db }
    }
}

#[async_trait(?Send)]
impl MagicLinkStore for D1MagicLinkStore {
    async fn create_magic_token(
        &self,
        email: &str,
        expires_at_unix: i64,
    ) -> Result<(String, String), ServerCoreError> {
        let token = uuid::Uuid::new_v4().to_string();
        let code = format!("{:06}", rand::random::<u32>() % 1_000_000);
        let now = chrono::Utc::now().timestamp();
        self.db
            .prepare(
                "INSERT INTO magic_tokens (token, email, code, expires_at, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )
            .bind(&[
                token.as_str().into(),
                email.into(),
                code.as_str().into(),
                ts(expires_at_unix),
                ts(now),
            ])
            .map_err(e)?
            .run()
            .await
            .map_err(e)?;
        Ok((token, code))
    }

    async fn peek_magic_token(&self, token: &str) -> Result<Option<String>, ServerCoreError> {
        let now = chrono::Utc::now().timestamp();
        let result = self
            .db
            .prepare(
                "SELECT email FROM magic_tokens WHERE token = ?1 AND used = 0 AND expires_at > ?2",
            )
            .bind(&[token.into(), ts(now)])
            .map_err(e)?
            .first::<serde_json::Value>(None)
            .await
            .map_err(e)?;
        Ok(result.and_then(|row| row["email"].as_str().map(|s| s.to_string())))
    }

    async fn consume_magic_token(&self, token: &str) -> Result<(), ServerCoreError> {
        self.db
            .prepare("UPDATE magic_tokens SET used = 1 WHERE token = ?1")
            .bind(&[token.into()])
            .map_err(e)?
            .run()
            .await
            .map_err(e)?;
        Ok(())
    }

    async fn peek_magic_code(
        &self,
        code: &str,
        email: &str,
    ) -> Result<Option<String>, ServerCoreError> {
        let now = chrono::Utc::now().timestamp();
        let result = self
            .db
            .prepare(
                "SELECT token FROM magic_tokens \
                 WHERE code = ?1 AND email = ?2 AND used = 0 AND expires_at > ?3",
            )
            .bind(&[code.into(), email.into(), ts(now)])
            .map_err(e)?
            .first::<serde_json::Value>(None)
            .await
            .map_err(e)?;
        Ok(result.map(|_| email.to_string()))
    }

    async fn consume_magic_code(&self, code: &str, email: &str) -> Result<(), ServerCoreError> {
        self.db
            .prepare("UPDATE magic_tokens SET used = 1 WHERE code = ?1 AND email = ?2")
            .bind(&[code.into(), email.into()])
            .map_err(e)?
            .run()
            .await
            .map_err(e)?;
        Ok(())
    }

    async fn count_recent_magic_tokens(
        &self,
        email: &str,
        since_unix: i64,
    ) -> Result<u64, ServerCoreError> {
        let result = self
            .db
            .prepare(
                "SELECT COUNT(*) as cnt FROM magic_tokens \
                 WHERE email = ?1 AND created_at > ?2",
            )
            .bind(&[email.into(), ts(since_unix)])
            .map_err(e)?
            .first::<serde_json::Value>(None)
            .await
            .map_err(e)?;
        Ok(result.and_then(|v| v["cnt"].as_u64()).unwrap_or(0))
    }
}

// ---------------------------------------------------------------------------
// UserStore
// ---------------------------------------------------------------------------

pub struct D1UserStore {
    db: D1Database,
}

impl D1UserStore {
    pub fn new(db: D1Database) -> Self {
        Self { db }
    }
}

#[async_trait(?Send)]
impl UserStore for D1UserStore {
    async fn get_or_create_user(&self, email: &str) -> Result<String, ServerCoreError> {
        // Try to find existing user
        let existing = self
            .db
            .prepare("SELECT id FROM users WHERE email = ?1")
            .bind(&[email.into()])
            .map_err(e)?
            .first::<serde_json::Value>(None)
            .await
            .map_err(e)?;

        if let Some(row) = existing {
            return Ok(row["id"].as_str().unwrap_or_default().to_string());
        }

        // Create new user
        let user_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp();
        self.db
            .prepare("INSERT INTO users (id, email, created_at, tier) VALUES (?1, ?2, ?3, 'free')")
            .bind(&[user_id.as_str().into(), email.into(), ts(now)])
            .map_err(e)?
            .run()
            .await
            .map_err(e)?;
        Ok(user_id)
    }

    async fn update_last_login(&self, user_id: &str) -> Result<(), ServerCoreError> {
        let now = chrono::Utc::now().timestamp();
        self.db
            .prepare("UPDATE users SET last_login_at = ?1 WHERE id = ?2")
            .bind(&[ts(now), user_id.into()])
            .map_err(e)?
            .run()
            .await
            .map_err(e)?;
        Ok(())
    }

    async fn delete_user(&self, user_id: &str) -> Result<(), ServerCoreError> {
        self.db
            .prepare("DELETE FROM users WHERE id = ?1")
            .bind(&[user_id.into()])
            .map_err(e)?
            .run()
            .await
            .map_err(e)?;
        Ok(())
    }

    async fn get_effective_device_limit(&self, user_id: &str) -> Result<u32, ServerCoreError> {
        let result = self
            .db
            .prepare("SELECT tier FROM users WHERE id = ?1")
            .bind(&[user_id.into()])
            .map_err(e)?
            .first::<serde_json::Value>(None)
            .await
            .map_err(e)?;

        let tier = result
            .map(|row| UserTier::from_str_lossy(row["tier"].as_str().unwrap_or("free")))
            .unwrap_or(UserTier::Free);
        Ok(tier.defaults().device_limit)
    }

    async fn set_user_tier(&self, user_id: &str, tier: UserTier) -> Result<(), ServerCoreError> {
        self.db
            .prepare("UPDATE users SET tier = ?1 WHERE id = ?2")
            .bind(&[tier.as_str().into(), user_id.into()])
            .map_err(e)?
            .run()
            .await
            .map_err(e)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// DeviceStore
// ---------------------------------------------------------------------------

pub struct D1DeviceStore {
    db: D1Database,
}

impl D1DeviceStore {
    pub fn new(db: D1Database) -> Self {
        Self { db }
    }
}

#[async_trait(?Send)]
impl DeviceStore for D1DeviceStore {
    async fn create_device(
        &self,
        user_id: &str,
        name: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<String, ServerCoreError> {
        let device_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp();
        self.db
            .prepare(
                "INSERT INTO devices (id, user_id, name, user_agent, created_at, last_seen_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .bind(&[
                device_id.as_str().into(),
                user_id.into(),
                name.unwrap_or("").into(),
                user_agent.unwrap_or("").into(),
                ts(now),
                ts(now),
            ])
            .map_err(e)?
            .run()
            .await
            .map_err(e)?;
        Ok(device_id)
    }

    async fn count_user_devices(&self, user_id: &str) -> Result<u32, ServerCoreError> {
        let result = self
            .db
            .prepare("SELECT COUNT(*) as cnt FROM devices WHERE user_id = ?1")
            .bind(&[user_id.into()])
            .map_err(e)?
            .first::<serde_json::Value>(None)
            .await
            .map_err(e)?;
        Ok(result.and_then(|v| v["cnt"].as_u64()).unwrap_or(0) as u32)
    }

    async fn list_user_devices(&self, user_id: &str) -> Result<Vec<DeviceInfo>, ServerCoreError> {
        let results = self
            .db
            .prepare(
                "SELECT id, user_id, name, user_agent, created_at, last_seen_at \
                 FROM devices WHERE user_id = ?1 ORDER BY last_seen_at DESC",
            )
            .bind(&[user_id.into()])
            .map_err(e)?
            .all()
            .await
            .map_err(e)?;

        let rows: Vec<serde_json::Value> = results.results().map_err(e)?;
        Ok(rows
            .into_iter()
            .map(|row| DeviceInfo {
                id: row["id"].as_str().unwrap_or_default().to_string(),
                user_id: row["user_id"].as_str().unwrap_or_default().to_string(),
                name: row["name"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string()),
                user_agent: row["user_agent"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string()),
                created_at: chrono::DateTime::from_timestamp(
                    row["created_at"].as_i64().unwrap_or_default(),
                    0,
                )
                .unwrap_or_else(chrono::Utc::now),
                last_seen_at: chrono::DateTime::from_timestamp(
                    row["last_seen_at"].as_i64().unwrap_or_default(),
                    0,
                )
                .unwrap_or_else(chrono::Utc::now),
            })
            .collect())
    }

    async fn delete_device(&self, device_id: &str) -> Result<(), ServerCoreError> {
        self.db
            .prepare("DELETE FROM devices WHERE id = ?1")
            .bind(&[device_id.into()])
            .map_err(e)?
            .run()
            .await
            .map_err(e)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn generate_session_code() -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let part = || -> String {
        (0..8)
            .map(|_| {
                let idx = rand::random::<usize>() % CHARSET.len();
                CHARSET[idx] as char
            })
            .collect()
    };
    format!("{}-{}", part(), part())
}
