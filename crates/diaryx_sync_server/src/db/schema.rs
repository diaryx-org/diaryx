use diaryx_server::schema::{CURRENT_VERSION, MIGRATIONS};
use rusqlite::Connection;

/// Initialize (or migrate) the database schema.
///
/// Uses `PRAGMA user_version` to track which migrations have been applied:
/// - **Fresh database** (user_version = 0, no tables): applies all canonical
///   migrations from `diaryx_server::schema`.
/// - **Legacy database** (user_version = 0, tables exist): runs
///   backwards-compatible migration code, then sets user_version.
/// - **Versioned database** (user_version > 0): applies only migrations
///   newer than the recorded version.
pub fn init_database(conn: &Connection) -> Result<(), rusqlite::Error> {
    let version: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

    if version == 0 {
        let is_legacy = conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='users' LIMIT 1",
                [],
                |_| Ok(()),
            )
            .is_ok();

        if is_legacy {
            legacy_migrate(conn)?;
        } else {
            for m in MIGRATIONS {
                conn.execute_batch(m.sql)?;
            }
        }
    } else {
        for m in MIGRATIONS {
            if m.version > version {
                conn.execute_batch(m.sql)?;
            }
        }
    }

    conn.pragma_update(None, "user_version", CURRENT_VERSION)?;
    Ok(())
}

/// Bring a pre-versioned (legacy) database up to the current schema.
///
/// This handles all the incremental ALTER TABLE / CREATE TABLE IF NOT EXISTS
/// changes that were added over time before we had version tracking.
/// Once a legacy DB passes through this function and gets a user_version set,
/// future upgrades use the clean migration path above.
fn legacy_migrate(conn: &Connection) -> Result<(), rusqlite::Error> {
    // Add missing columns to existing tables BEFORE applying the initial
    // migration, because the initial migration creates indexes that reference
    // columns which may not exist in very old legacy databases.

    // -- Column additions on `users` --
    // Order must match the CREATE TABLE in 0001_initial.sql so that
    // legacy and fresh installs produce identical column orderings.
    add_column_if_missing(conn, "users", "attachment_limit_bytes", "INTEGER")?;
    add_column_if_missing(conn, "users", "workspace_limit", "INTEGER")?;
    add_column_if_missing(conn, "users", "tier", "TEXT NOT NULL DEFAULT 'free'")?;
    add_column_if_missing(conn, "users", "device_limit", "INTEGER")?;
    add_column_if_missing(conn, "users", "published_site_limit", "INTEGER")?;
    add_column_if_missing(conn, "users", "stripe_customer_id", "TEXT")?;
    add_column_if_missing(conn, "users", "stripe_subscription_id", "TEXT")?;
    add_column_if_missing(conn, "users", "apple_original_transaction_id", "TEXT")?;

    // -- Column additions on `magic_tokens` (table may not exist yet) --
    if table_exists(conn, "magic_tokens") {
        add_column_if_missing(conn, "magic_tokens", "code", "TEXT")?;
    }

    // Now apply the initial schema idempotently — CREATE TABLE IF NOT EXISTS
    // will skip tables that already exist, CREATE INDEX IF NOT EXISTS likewise.
    conn.execute_batch(MIGRATIONS[0].sql)?;

    // -- Column additions on `namespace_objects` (created by migration 1) --
    add_column_if_missing(conn, "namespace_objects", "audience", "TEXT")?;
    add_column_if_missing(conn, "namespace_objects", "content_hash", "TEXT")?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_namespace_objects_audience \
         ON namespace_objects(namespace_id, audience);",
    )?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_namespace_objects_r2_key \
         ON namespace_objects(namespace_id, r2_key);",
    )?;

    // -- Column additions on `namespaces` --
    add_column_if_missing(conn, "namespaces", "metadata", "TEXT")?;

    // -- Backfill: clear explicit 200 MiB limits so tier defaults take effect --
    conn.execute(
        "UPDATE users SET attachment_limit_bytes = NULL WHERE attachment_limit_bytes = 209715200",
        [],
    )?;

    Ok(())
}

fn table_exists(conn: &Connection, table: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1 LIMIT 1",
        [table],
        |_| Ok(()),
    )
    .is_ok()
}

/// Add a column to a table if it doesn't already exist.
fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    col_type: &str,
) -> Result<(), rusqlite::Error> {
    let exists: bool = conn
        .prepare(&format!("PRAGMA table_info('{table}')"))?
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(Result::ok)
        .any(|name| name == column);
    if !exists {
        conn.execute_batch(&format!(
            "ALTER TABLE {table} ADD COLUMN {column} {col_type}"
        ))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_database() {
        let conn = Connection::open_in_memory().unwrap();
        init_database(&conn).unwrap();
        assert_version_and_tables(&conn);
    }

    #[test]
    fn legacy_database_migrates() {
        let conn = Connection::open_in_memory().unwrap();
        // Simulate an early legacy DB with only the bare users table.
        conn.execute_batch(
            r#"
            CREATE TABLE users (
                id TEXT PRIMARY KEY,
                email TEXT UNIQUE NOT NULL,
                created_at INTEGER NOT NULL,
                last_login_at INTEGER
            );
            INSERT INTO users (id, email, created_at) VALUES ('u1', 'u1@example.com', 1);
            "#,
        )
        .unwrap();

        init_database(&conn).unwrap();
        assert_version_and_tables(&conn);

        // Verify the tier column was added with default.
        let tier: String = conn
            .query_row("SELECT tier FROM users WHERE id = 'u1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(tier, "free");
    }

    #[test]
    fn idempotent_re_init() {
        let conn = Connection::open_in_memory().unwrap();
        init_database(&conn).unwrap();
        // Running again should be a no-op.
        init_database(&conn).unwrap();
        assert_version_and_tables(&conn);
    }

    #[test]
    fn backfill_clears_old_200mib_default() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE users (
                id TEXT PRIMARY KEY,
                email TEXT UNIQUE NOT NULL,
                created_at INTEGER NOT NULL,
                last_login_at INTEGER,
                attachment_limit_bytes INTEGER
            );
            INSERT INTO users (id, email, created_at, attachment_limit_bytes)
                VALUES ('u1', 'u1@example.com', 1, 209715200);
            INSERT INTO users (id, email, created_at, attachment_limit_bytes)
                VALUES ('u2', 'u2@example.com', 1, 500000000);
            "#,
        )
        .unwrap();

        init_database(&conn).unwrap();

        let u1_limit: Option<i64> = conn
            .query_row(
                "SELECT attachment_limit_bytes FROM users WHERE id = 'u1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(u1_limit.is_none());

        let u2_limit: Option<i64> = conn
            .query_row(
                "SELECT attachment_limit_bytes FROM users WHERE id = 'u2'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(u2_limit, Some(500_000_000));
    }

    #[test]
    fn legacy_and_fresh_produce_same_schema() {
        // Fresh install.
        let fresh = Connection::open_in_memory().unwrap();
        init_database(&fresh).unwrap();

        // Legacy: start from bare users table.
        let legacy = Connection::open_in_memory().unwrap();
        legacy
            .execute_batch(
                "CREATE TABLE users (\
                     id TEXT PRIMARY KEY, \
                     email TEXT UNIQUE NOT NULL, \
                     created_at INTEGER NOT NULL, \
                     last_login_at INTEGER\
                 );",
            )
            .unwrap();
        init_database(&legacy).unwrap();

        let fresh_tables = get_table_columns(&fresh);
        let legacy_tables = get_table_columns(&legacy);
        assert_eq!(fresh_tables, legacy_tables);
    }

    // -- helpers --

    fn assert_version_and_tables(conn: &Connection) {
        let version: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, CURRENT_VERSION);

        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(Result::ok)
            .collect();

        for expected in [
            "auth_sessions",
            "devices",
            "magic_tokens",
            "namespaces",
            "namespace_objects",
            "namespace_audiences",
            "namespace_sessions",
            "passkey_credentials",
            "passkey_challenges",
            "usage_events",
            "user_ai_usage_monthly",
            "users",
        ] {
            assert!(
                tables.contains(&expected.to_string()),
                "missing table: {expected}"
            );
        }

        // Verify columns from later migrations.
        let ns_cols = column_names(conn, "namespaces");
        assert!(ns_cols.contains(&"metadata".to_string()));

        let obj_cols = column_names(conn, "namespace_objects");
        assert!(obj_cols.contains(&"content_hash".to_string()));
    }

    fn column_names(conn: &Connection, table: &str) -> Vec<String> {
        conn.prepare(&format!("PRAGMA table_info('{table}')"))
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .filter_map(Result::ok)
            .collect()
    }

    fn get_table_columns(
        conn: &Connection,
    ) -> std::collections::BTreeMap<String, Vec<(String, String, bool, bool)>> {
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
                let cols: Vec<(String, String, bool, bool)> = conn
                    .prepare(&format!("PRAGMA table_info('{t}')"))
                    .unwrap()
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, bool>(3)?,
                            row.get::<_, bool>(5)?,
                        ))
                    })
                    .unwrap()
                    .filter_map(Result::ok)
                    .collect();
                (t, cols)
            })
            .collect()
    }
}
