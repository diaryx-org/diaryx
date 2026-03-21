//! CrdtStorage implementation backed by Durable Object's embedded SQLite.
//!
//! Uses the same schema as `diaryx_sync::SqliteStorage` but accessed through
//! the worker crate's `SqlStorage` API (synchronous within the DO context).

use diaryx_sync::{CrdtStorage, CrdtUpdate, StorageResult, UpdateOrigin};
use worker::{SqlStorage, SqlStorageValue};
use yrs::{ReadTxn, Transact, updates::decoder::Decode, updates::encoder::Encode};

fn e(err: impl std::fmt::Display) -> diaryx_core::error::DiaryxError {
    diaryx_core::error::DiaryxError::Validation(format!("DO SQL error: {err}"))
}

/// CrdtStorage backed by a Durable Object's embedded SQLite.
pub struct DoSyncStorage {
    sql: SqlStorage,
}

impl DoSyncStorage {
    pub fn new(sql: SqlStorage) -> Self {
        Self { sql }
    }

    /// Initialize the CRDT schema tables.
    pub fn init_schema(&self) -> StorageResult<()> {
        self.sql
            .exec(
                "CREATE TABLE IF NOT EXISTS documents (
                    name TEXT PRIMARY KEY,
                    state BLOB NOT NULL,
                    state_vector BLOB NOT NULL,
                    updated_at INTEGER NOT NULL
                )",
                vec![],
            )
            .map_err(e)?;
        self.sql
            .exec(
                "CREATE TABLE IF NOT EXISTS updates (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    doc_name TEXT NOT NULL,
                    data BLOB NOT NULL,
                    origin TEXT NOT NULL,
                    timestamp INTEGER NOT NULL,
                    device_id TEXT,
                    device_name TEXT
                )",
                vec![],
            )
            .map_err(e)?;
        self.sql
            .exec(
                "CREATE INDEX IF NOT EXISTS idx_updates_doc_id ON updates(doc_name, id)",
                vec![],
            )
            .map_err(e)?;
        Ok(())
    }

    fn now_ms(&self) -> i64 {
        chrono::Utc::now().timestamp_millis()
    }
}

/// Row type for document queries.
#[derive(serde::Deserialize)]
struct DocRow {
    state: serde_bytes::ByteBuf,
}

/// Row type for update queries.
#[derive(serde::Deserialize)]
struct UpdateRow {
    id: i64,
    doc_name: String,
    data: serde_bytes::ByteBuf,
    origin: String,
    timestamp: i64,
    device_id: Option<String>,
    device_name: Option<String>,
}

/// Row type for ID-only queries.
#[derive(serde::Deserialize)]
struct IdRow {
    id: i64,
}

/// Row type for name queries.
#[derive(serde::Deserialize)]
struct NameRow {
    name: String,
}

impl CrdtStorage for DoSyncStorage {
    fn load_doc(&self, name: &str) -> StorageResult<Option<Vec<u8>>> {
        let cursor = self
            .sql
            .exec(
                "SELECT state FROM documents WHERE name = ?",
                vec![name.into()],
            )
            .map_err(e)?;

        match cursor.one::<DocRow>() {
            Ok(row) => Ok(Some(row.state.into_vec())),
            Err(_) => Ok(None),
        }
    }

    fn save_doc(&self, name: &str, state: &[u8]) -> StorageResult<()> {
        let now = self.now_ms();
        // Compute state vector from the state
        let sv = yrs::Doc::new();
        let sv_bytes = {
            let mut txn = sv.transact_mut();
            if let Ok(update) = yrs::Update::decode_v1(state) {
                let _ = txn.apply_update(update);
            }
            drop(txn);
            let txn = sv.transact();
            txn.state_vector().encode_v1()
        };

        self.sql
            .exec(
                "INSERT OR REPLACE INTO documents (name, state, state_vector, updated_at) VALUES (?, ?, ?, ?)",
                vec![
                    name.into(),
                    SqlStorageValue::Blob(state.to_vec()),
                    SqlStorageValue::Blob(sv_bytes),
                    now.into(),
                ],
            )
            .map_err(e)?;
        Ok(())
    }

    fn delete_doc(&self, name: &str) -> StorageResult<()> {
        self.sql
            .exec("DELETE FROM documents WHERE name = ?", vec![name.into()])
            .map_err(e)?;
        self.sql
            .exec("DELETE FROM updates WHERE doc_name = ?", vec![name.into()])
            .map_err(e)?;
        Ok(())
    }

    fn list_docs(&self) -> StorageResult<Vec<String>> {
        let cursor = self
            .sql
            .exec("SELECT name FROM documents ORDER BY name", vec![])
            .map_err(e)?;
        let rows: Vec<NameRow> = cursor.to_array().map_err(e)?;
        Ok(rows.into_iter().map(|r| r.name).collect())
    }

    fn append_update_with_device(
        &self,
        name: &str,
        update: &[u8],
        origin: UpdateOrigin,
        device_id: Option<&str>,
        device_name: Option<&str>,
    ) -> StorageResult<i64> {
        let now = self.now_ms();
        self.sql
            .exec(
                "INSERT INTO updates (doc_name, data, origin, timestamp, device_id, device_name) VALUES (?, ?, ?, ?, ?, ?)",
                vec![
                    name.into(),
                    SqlStorageValue::Blob(update.to_vec()),
                    origin.to_string().into(),
                    now.into(),
                    device_id.map(|s| s.to_string().into()).unwrap_or(SqlStorageValue::Null),
                    device_name.map(|s| s.to_string().into()).unwrap_or(SqlStorageValue::Null),
                ],
            )
            .map_err(e)?;

        // Get last insert rowid
        let cursor = self
            .sql
            .exec("SELECT last_insert_rowid() as id", vec![])
            .map_err(e)?;
        let row: IdRow = cursor.one().map_err(e)?;
        Ok(row.id)
    }

    fn get_updates_since(&self, name: &str, since_id: i64) -> StorageResult<Vec<CrdtUpdate>> {
        let cursor = self
            .sql
            .exec(
                "SELECT id, doc_name, data, origin, timestamp, device_id, device_name FROM updates WHERE doc_name = ? AND id > ? ORDER BY id",
                vec![name.into(), since_id.into()],
            )
            .map_err(e)?;
        let rows: Vec<UpdateRow> = cursor.to_array().map_err(e)?;
        Ok(rows
            .into_iter()
            .map(|r| CrdtUpdate {
                update_id: r.id,
                doc_name: r.doc_name,
                data: r.data.into_vec(),
                timestamp: r.timestamp,
                origin: r.origin.parse().unwrap_or(UpdateOrigin::Remote),
                device_id: r.device_id,
                device_name: r.device_name,
            })
            .collect())
    }

    fn get_all_updates(&self, name: &str) -> StorageResult<Vec<CrdtUpdate>> {
        self.get_updates_since(name, 0)
    }

    fn get_state_at(&self, name: &str, update_id: i64) -> StorageResult<Option<Vec<u8>>> {
        let base = self.load_doc(name)?;
        let updates = self.get_updates_since(name, 0)?;

        let doc = yrs::Doc::new();
        {
            let mut txn = doc.transact_mut();
            if let Some(state) = &base {
                if let Ok(update) = yrs::Update::decode_v1(state) {
                    let _ = txn.apply_update(update);
                }
            }
            for u in &updates {
                if u.update_id > update_id {
                    break;
                }
                if let Ok(update) = yrs::Update::decode_v1(&u.data) {
                    let _ = txn.apply_update(update);
                }
            }
        }
        let state = doc
            .transact()
            .encode_state_as_update_v1(&yrs::StateVector::default());
        Ok(Some(state))
    }

    fn compact(&self, name: &str, keep_updates: usize) -> StorageResult<()> {
        // Load current state, save as snapshot, remove old updates
        let updates = self.get_all_updates(name)?;
        if updates.len() <= keep_updates {
            return Ok(());
        }

        // Merge all into snapshot
        let doc = yrs::Doc::new();
        {
            let mut txn = doc.transact_mut();
            if let Some(state) = self.load_doc(name)? {
                if let Ok(update) = yrs::Update::decode_v1(&state) {
                    let _ = txn.apply_update(update);
                }
            }
            for u in &updates {
                if let Ok(update) = yrs::Update::decode_v1(&u.data) {
                    let _ = txn.apply_update(update);
                }
            }
        }
        let state = doc
            .transact()
            .encode_state_as_update_v1(&yrs::StateVector::default());
        self.save_doc(name, &state)?;

        // Remove old updates, keep only the most recent
        let cutoff_id = updates[updates.len() - keep_updates].update_id;
        self.sql
            .exec(
                "DELETE FROM updates WHERE doc_name = ? AND id < ?",
                vec![name.into(), cutoff_id.into()],
            )
            .map_err(e)?;
        Ok(())
    }

    fn get_latest_update_id(&self, name: &str) -> StorageResult<i64> {
        let cursor = self
            .sql
            .exec(
                "SELECT COALESCE(MAX(id), 0) as id FROM updates WHERE doc_name = ?",
                vec![name.into()],
            )
            .map_err(e)?;
        let row: IdRow = cursor.one().map_err(e)?;
        Ok(row.id)
    }

    fn rename_doc(&self, old_name: &str, new_name: &str) -> StorageResult<()> {
        // Copy document
        if let Some(state) = self.load_doc(old_name)? {
            self.save_doc(new_name, &state)?;
        }
        // Copy updates
        self.sql
            .exec(
                "INSERT INTO updates (doc_name, data, origin, timestamp, device_id, device_name)
                 SELECT ?, data, origin, timestamp, device_id, device_name FROM updates WHERE doc_name = ? ORDER BY id",
                vec![new_name.into(), old_name.into()],
            )
            .map_err(e)?;
        // Delete old
        self.delete_doc(old_name)?;
        Ok(())
    }

    fn clear_updates(&self, name: &str) -> StorageResult<()> {
        self.sql
            .exec("DELETE FROM updates WHERE doc_name = ?", vec![name.into()])
            .map_err(e)?;
        Ok(())
    }
}
