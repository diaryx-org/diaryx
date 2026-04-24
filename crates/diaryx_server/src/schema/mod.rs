//! Canonical database migrations for Diaryx server adapters (SQLite dialect).
//!
//! Both the native sync server (rusqlite) and Cloudflare Workers (D1) consume
//! these migrations. Other SQL engines should use the port traits in [`crate::ports`]
//! as the authoritative contract and treat these migrations as a reference
//! implementation.

/// A numbered schema migration.
pub struct Migration {
    /// Monotonically increasing version number (1-based).
    pub version: u32,
    /// Human-readable name.
    pub name: &'static str,
    /// SQL to apply (may contain multiple statements).
    pub sql: &'static str,
}

/// Ordered list of all migrations. Applying them sequentially to an empty
/// database produces the current target schema.
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial",
        sql: include_str!("0001_initial.sql"),
    },
    Migration {
        version: 2,
        name: "content_hash",
        sql: include_str!("0002_content_hash.sql"),
    },
    Migration {
        version: 3,
        name: "namespace_metadata",
        sql: include_str!("0003_namespace_metadata.sql"),
    },
    Migration {
        version: 4,
        name: "audience_gates",
        sql: include_str!("0004_audience_gates.sql"),
    },
];

/// The version number of the latest migration.
pub const CURRENT_VERSION: u32 = 4;

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::collections::BTreeMap;

    #[test]
    fn migrations_are_sequential() {
        for (i, m) in MIGRATIONS.iter().enumerate() {
            assert_eq!(
                m.version,
                (i + 1) as u32,
                "migration '{}' has wrong version",
                m.name
            );
        }
        assert_eq!(MIGRATIONS.last().unwrap().version, CURRENT_VERSION);
    }

    #[test]
    fn migrations_apply_cleanly() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        for m in MIGRATIONS {
            conn.execute_batch(m.sql)
                .unwrap_or_else(|e| panic!("migration '{}' failed: {}", m.name, e));
        }

        let expected_tables = [
            "auth_sessions",
            "custom_domains",
            "devices",
            "magic_tokens",
            "namespace_audiences",
            "namespace_objects",
            "namespace_sessions",
            "namespaces",
            "passkey_challenges",
            "passkey_credentials",
            "usage_events",
            "user_ai_usage_monthly",
            "users",
        ];

        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(Result::ok)
            .collect();

        for t in &expected_tables {
            assert!(tables.contains(&t.to_string()), "missing table: {t}");
        }

        // Verify key columns added by later migrations exist.
        let ns_obj_cols = get_columns(&conn, "namespace_objects");
        assert!(
            ns_obj_cols.contains_key("content_hash"),
            "missing content_hash column"
        );

        let ns_cols = get_columns(&conn, "namespaces");
        assert!(ns_cols.contains_key("metadata"), "missing metadata column");
    }

    #[test]
    fn cloudflare_migrations_match() {
        let canonical = apply_canonical();
        let cf = apply_cf_migrations();

        let canonical_schema = snapshot_schema(&canonical);
        let cf_schema = snapshot_schema(&cf);

        assert_eq!(
            canonical_schema,
            cf_schema,
            "CF migrations diverge from canonical schema.\n\
             Canonical tables: {:?}\n\
             CF tables: {:?}",
            canonical_schema.keys().collect::<Vec<_>>(),
            cf_schema.keys().collect::<Vec<_>>(),
        );
    }

    // -- helpers --

    fn apply_canonical() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        for m in MIGRATIONS {
            conn.execute_batch(m.sql).unwrap();
        }
        conn
    }

    fn apply_cf_migrations() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        // Read CF migration files from the sibling crate.
        let cf_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../diaryx_cloudflare/migrations");
        let mut files: Vec<_> = std::fs::read_dir(&cf_dir)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", cf_dir.display()))
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().map_or(false, |ext| ext == "sql"))
            .collect();
        files.sort_by_key(|e| e.file_name());
        assert!(!files.is_empty(), "no CF migration files found");
        for entry in &files {
            let sql = std::fs::read_to_string(entry.path()).unwrap();
            conn.execute_batch(&sql)
                .unwrap_or_else(|e| panic!("{}: {e}", entry.path().display()));
        }
        conn
    }

    /// Column name → (type, notnull, pk).
    type ColumnMap = BTreeMap<String, (String, bool, bool)>;

    fn get_columns(conn: &Connection, table: &str) -> ColumnMap {
        conn.prepare(&format!("PRAGMA table_info('{table}')"))
            .unwrap()
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(1)?, // name
                    row.get::<_, String>(2)?, // type
                    row.get::<_, bool>(3)?,   // notnull
                    row.get::<_, bool>(5)?,   // pk
                ))
            })
            .unwrap()
            .filter_map(Result::ok)
            .map(|(name, ty, notnull, pk)| (name, (ty, notnull, pk)))
            .collect()
    }

    /// Table name → columns.
    type SchemaSnapshot = BTreeMap<String, ColumnMap>;

    fn snapshot_schema(conn: &Connection) -> SchemaSnapshot {
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        tables
            .into_iter()
            .map(|t| {
                let cols = get_columns(conn, &t);
                (t, cols)
            })
            .collect()
    }
}
