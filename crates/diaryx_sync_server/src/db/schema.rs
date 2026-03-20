use rusqlite::Connection;

/// SQL schema for auth-related tables
const SCHEMA: &str = r#"
-- Users table
CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    email TEXT UNIQUE NOT NULL,
    created_at INTEGER NOT NULL,
    last_login_at INTEGER,
    attachment_limit_bytes INTEGER,
    workspace_limit INTEGER,
    tier TEXT NOT NULL DEFAULT 'free',
    device_limit INTEGER,
    published_site_limit INTEGER,
    stripe_customer_id TEXT,
    stripe_subscription_id TEXT,
    apple_original_transaction_id TEXT
);

-- Devices table (tracks client devices)
CREATE TABLE IF NOT EXISTS devices (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT,
    user_agent TEXT,
    created_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_devices_user_id ON devices(user_id);

-- Magic link tokens (short-lived, for email verification)
CREATE TABLE IF NOT EXISTS magic_tokens (
    token TEXT PRIMARY KEY,
    email TEXT NOT NULL,
    code TEXT,
    expires_at INTEGER NOT NULL,
    used INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_magic_tokens_email ON magic_tokens(email);
CREATE INDEX IF NOT EXISTS idx_magic_tokens_expires ON magic_tokens(expires_at);

-- Auth sessions (long-lived, for authenticated access)
CREATE TABLE IF NOT EXISTS auth_sessions (
    token TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    device_id TEXT NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    expires_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_sessions_user_id ON auth_sessions(user_id);
CREATE INDEX IF NOT EXISTS idx_sessions_expires ON auth_sessions(expires_at);

-- Managed AI monthly usage counters (per-user, per UTC month).
CREATE TABLE IF NOT EXISTS user_ai_usage_monthly (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    period_utc TEXT NOT NULL, -- YYYY-MM in UTC
    request_count INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, period_utc)
);

CREATE INDEX IF NOT EXISTS idx_user_ai_usage_monthly_user ON user_ai_usage_monthly(user_id);

-- ============================================================
-- Generic namespace / object store / audience tables
-- (greenfield server primitives)
-- ============================================================

-- User-owned namespaces.  Convention: id = "workspace:{uuid}" | "site:{uuid}".
-- No semantic type enforced by server — plugins use naming conventions.
CREATE TABLE IF NOT EXISTS namespaces (
    id TEXT PRIMARY KEY,
    owner_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_namespaces_owner ON namespaces(owner_user_id);

-- Generic key→bytes object store within a namespace.
-- Objects are backed by R2; the data column stores the R2 object key, not the
-- bytes directly, to keep SQLite lean.  A NULL r2_key means inline storage for
-- small objects (≤ 64 KiB) kept in the data column.
CREATE TABLE IF NOT EXISTS namespace_objects (
    namespace_id TEXT NOT NULL REFERENCES namespaces(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    r2_key TEXT,                          -- NULL = inline
    data BLOB,                            -- non-NULL when r2_key IS NULL
    mime_type TEXT NOT NULL DEFAULT 'application/octet-stream',
    size_bytes INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    audience TEXT,                         -- NULL = private (owner-only); non-NULL refs namespace_audiences
    PRIMARY KEY (namespace_id, key)
);

CREATE INDEX IF NOT EXISTS idx_namespace_objects_audience ON namespace_objects(namespace_id, audience);

CREATE INDEX IF NOT EXISTS idx_namespace_objects_ns ON namespace_objects(namespace_id);

-- Audience visibility records per namespace.
CREATE TABLE IF NOT EXISTS namespace_audiences (
    namespace_id TEXT NOT NULL REFERENCES namespaces(id) ON DELETE CASCADE,
    audience_name TEXT NOT NULL,
    access TEXT NOT NULL DEFAULT 'private', -- 'public' | 'token' | 'private'
    PRIMARY KEY (namespace_id, audience_name)
);

CREATE INDEX IF NOT EXISTS idx_namespace_audiences_ns ON namespace_audiences(namespace_id);

-- Custom domain → namespace+audience mapping for Caddy forward_auth.
CREATE TABLE IF NOT EXISTS custom_domains (
    domain TEXT PRIMARY KEY,
    namespace_id TEXT NOT NULL REFERENCES namespaces(id) ON DELETE CASCADE,
    audience_name TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    verified INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_custom_domains_ns ON custom_domains(namespace_id);

-- Usage events for metering (bytes_in, bytes_out, bytes_stored, relay_seconds).
CREATE TABLE IF NOT EXISTS usage_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL,   -- 'bytes_in' | 'bytes_out' | 'relay_seconds'
    amount INTEGER NOT NULL,
    namespace_id TEXT,          -- optional context
    recorded_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_usage_events_user ON usage_events(user_id, event_type, recorded_at);
CREATE INDEX IF NOT EXISTS idx_usage_events_recorded ON usage_events(recorded_at);

-- Namespace-scoped sessions (generic share sessions for any namespace).
CREATE TABLE IF NOT EXISTS namespace_sessions (
    code TEXT PRIMARY KEY,
    namespace_id TEXT NOT NULL REFERENCES namespaces(id) ON DELETE CASCADE,
    owner_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    read_only INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    expires_at INTEGER
);

CREATE INDEX IF NOT EXISTS idx_namespace_sessions_owner ON namespace_sessions(owner_user_id);
CREATE INDEX IF NOT EXISTS idx_namespace_sessions_ns ON namespace_sessions(namespace_id);

-- Passkey (WebAuthn) credentials for passwordless login.
CREATE TABLE IF NOT EXISTS passkey_credentials (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    credential_json TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    last_used_at INTEGER
);

CREATE INDEX IF NOT EXISTS idx_passkey_credentials_user ON passkey_credentials(user_id);

-- Ephemeral passkey ceremony state (registration / authentication challenges).
CREATE TABLE IF NOT EXISTS passkey_challenges (
    challenge_id TEXT PRIMARY KEY,
    user_id TEXT,
    email TEXT NOT NULL,
    challenge_type TEXT NOT NULL,
    state_json TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_passkey_challenges_expires ON passkey_challenges(expires_at);
"#;

/// Initialize the database with the auth schema
pub fn init_database(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(SCHEMA)?;
    // Forward migration for older databases.
    let has_limit_col: bool = conn
        .prepare("PRAGMA table_info(users)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(Result::ok)
        .any(|name| name == "attachment_limit_bytes");
    if !has_limit_col {
        conn.execute(
            "ALTER TABLE users ADD COLUMN attachment_limit_bytes INTEGER",
            [],
        )?;
    }
    // Note: the old backfill that set attachment_limit_bytes = 209715200 for
    // NULL rows has been replaced by the tier system. The reverse backfill
    // above clears explicit 200 MiB limits so tier defaults take effect.

    // Forward migration: add workspace_limit column to users table.
    let has_workspace_limit_col: bool = conn
        .prepare("PRAGMA table_info(users)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(Result::ok)
        .any(|name| name == "workspace_limit");
    if !has_workspace_limit_col {
        conn.execute("ALTER TABLE users ADD COLUMN workspace_limit INTEGER", [])?;
    }

    // Forward migration: add tier column to users table.
    let has_tier_col: bool = conn
        .prepare("PRAGMA table_info(users)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(Result::ok)
        .any(|name| name == "tier");
    if !has_tier_col {
        conn.execute(
            "ALTER TABLE users ADD COLUMN tier TEXT NOT NULL DEFAULT 'free'",
            [],
        )?;
    }

    // Forward migration: add published_site_limit column to users table.
    let has_published_site_limit_col: bool = conn
        .prepare("PRAGMA table_info(users)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(Result::ok)
        .any(|name| name == "published_site_limit");
    if !has_published_site_limit_col {
        conn.execute(
            "ALTER TABLE users ADD COLUMN published_site_limit INTEGER",
            [],
        )?;
    }

    // Forward migration: add device_limit column to users table.
    let has_device_limit_col: bool = conn
        .prepare("PRAGMA table_info(users)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(Result::ok)
        .any(|name| name == "device_limit");
    if !has_device_limit_col {
        conn.execute("ALTER TABLE users ADD COLUMN device_limit INTEGER", [])?;
    }

    // Backfill: clear explicit attachment limits that match the old Free default (200 MiB)
    // so tier defaults take effect cleanly.
    conn.execute(
        "UPDATE users SET attachment_limit_bytes = NULL WHERE attachment_limit_bytes = 209715200",
        [],
    )?;

    // Forward migration: add stripe_customer_id column to users table.
    let has_stripe_customer_id_col: bool = conn
        .prepare("PRAGMA table_info(users)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(Result::ok)
        .any(|name| name == "stripe_customer_id");
    if !has_stripe_customer_id_col {
        conn.execute("ALTER TABLE users ADD COLUMN stripe_customer_id TEXT", [])?;
    }

    // Forward migration: add stripe_subscription_id column to users table.
    let has_stripe_subscription_id_col: bool = conn
        .prepare("PRAGMA table_info(users)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(Result::ok)
        .any(|name| name == "stripe_subscription_id");
    if !has_stripe_subscription_id_col {
        conn.execute(
            "ALTER TABLE users ADD COLUMN stripe_subscription_id TEXT",
            [],
        )?;
    }

    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_users_stripe_customer \
         ON users(stripe_customer_id) WHERE stripe_customer_id IS NOT NULL;",
    )?;

    // Forward migration: add apple_original_transaction_id column to users table.
    let has_apple_tx_col: bool = conn
        .prepare("PRAGMA table_info(users)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(Result::ok)
        .any(|name| name == "apple_original_transaction_id");
    if !has_apple_tx_col {
        conn.execute(
            "ALTER TABLE users ADD COLUMN apple_original_transaction_id TEXT",
            [],
        )?;
    }
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_users_apple_tx \
         ON users(apple_original_transaction_id) WHERE apple_original_transaction_id IS NOT NULL;",
    )?;

    // Forward migration: add code column to magic_tokens table.
    let has_code_col: bool = conn
        .prepare("PRAGMA table_info(magic_tokens)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(Result::ok)
        .any(|name| name == "code");
    if !has_code_col {
        conn.execute("ALTER TABLE magic_tokens ADD COLUMN code TEXT", [])?;
    }
    conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_magic_tokens_code ON magic_tokens(code);")?;

    // Forward migration: create namespace/object/audience/usage tables if missing.
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS namespaces (
            id TEXT PRIMARY KEY,
            owner_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            created_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_namespaces_owner ON namespaces(owner_user_id);

        CREATE TABLE IF NOT EXISTS namespace_objects (
            namespace_id TEXT NOT NULL REFERENCES namespaces(id) ON DELETE CASCADE,
            key TEXT NOT NULL,
            r2_key TEXT,
            data BLOB,
            mime_type TEXT NOT NULL DEFAULT 'application/octet-stream',
            size_bytes INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            audience TEXT,
            PRIMARY KEY (namespace_id, key)
        );
        CREATE INDEX IF NOT EXISTS idx_namespace_objects_ns ON namespace_objects(namespace_id);
        CREATE INDEX IF NOT EXISTS idx_namespace_objects_audience ON namespace_objects(namespace_id, audience);

        CREATE TABLE IF NOT EXISTS namespace_audiences (
            namespace_id TEXT NOT NULL REFERENCES namespaces(id) ON DELETE CASCADE,
            audience_name TEXT NOT NULL,
            access TEXT NOT NULL DEFAULT 'private',
            PRIMARY KEY (namespace_id, audience_name)
        );
        CREATE INDEX IF NOT EXISTS idx_namespace_audiences_ns ON namespace_audiences(namespace_id);

        CREATE TABLE IF NOT EXISTS usage_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            event_type TEXT NOT NULL,
            amount INTEGER NOT NULL,
            namespace_id TEXT,
            recorded_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_usage_events_user ON usage_events(user_id, event_type, recorded_at);
        CREATE INDEX IF NOT EXISTS idx_usage_events_recorded ON usage_events(recorded_at);

        CREATE TABLE IF NOT EXISTS namespace_sessions (
            code TEXT PRIMARY KEY,
            namespace_id TEXT NOT NULL REFERENCES namespaces(id) ON DELETE CASCADE,
            owner_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            read_only INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL,
            expires_at INTEGER
        );
        CREATE INDEX IF NOT EXISTS idx_namespace_sessions_owner ON namespace_sessions(owner_user_id);
        CREATE INDEX IF NOT EXISTS idx_namespace_sessions_ns ON namespace_sessions(namespace_id);
        "#,
    )?;

    // Forward migration: add audience column to namespace_objects.
    let has_audience_col: bool = conn
        .prepare("PRAGMA table_info(namespace_objects)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(Result::ok)
        .any(|name| name == "audience");
    if !has_audience_col {
        conn.execute("ALTER TABLE namespace_objects ADD COLUMN audience TEXT", [])?;
    }
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_namespace_objects_audience ON namespace_objects(namespace_id, audience);",
    )?;

    // Forward migration: create custom_domains table if missing.
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS custom_domains (
            domain TEXT PRIMARY KEY,
            namespace_id TEXT NOT NULL REFERENCES namespaces(id) ON DELETE CASCADE,
            audience_name TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            verified INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_custom_domains_ns ON custom_domains(namespace_id);
        "#,
    )?;

    // Forward migration: add content_hash column to namespace_objects.
    let has_content_hash_col: bool = conn
        .prepare("PRAGMA table_info(namespace_objects)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(Result::ok)
        .any(|name| name == "content_hash");
    if !has_content_hash_col {
        conn.execute(
            "ALTER TABLE namespace_objects ADD COLUMN content_hash TEXT",
            [],
        )?;
    }
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_namespace_objects_r2_key ON namespace_objects(namespace_id, r2_key);",
    )?;

    // Forward migration: create passkey tables if missing.
    let has_passkey_credentials = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='passkey_credentials' LIMIT 1",
            [],
            |_| Ok(()),
        )
        .is_ok();
    if !has_passkey_credentials {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS passkey_credentials (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                name TEXT NOT NULL,
                credential_json TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                last_used_at INTEGER
            );
            CREATE INDEX IF NOT EXISTS idx_passkey_credentials_user ON passkey_credentials(user_id);

            CREATE TABLE IF NOT EXISTS passkey_challenges (
                challenge_id TEXT PRIMARY KEY,
                user_id TEXT,
                email TEXT NOT NULL,
                challenge_type TEXT NOT NULL,
                state_json TEXT NOT NULL,
                expires_at INTEGER NOT NULL,
                created_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_passkey_challenges_expires ON passkey_challenges(expires_at);
            "#,
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_database() {
        let conn = Connection::open_in_memory().unwrap();
        init_database(&conn).unwrap();

        // Verify tables exist
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(tables.contains(&"users".to_string()));
        assert!(tables.contains(&"devices".to_string()));
        assert!(tables.contains(&"magic_tokens".to_string()));
        assert!(tables.contains(&"auth_sessions".to_string()));
        assert!(tables.contains(&"user_ai_usage_monthly".to_string()));
        assert!(tables.contains(&"passkey_credentials".to_string()));
        assert!(tables.contains(&"passkey_challenges".to_string()));
        assert!(tables.contains(&"namespaces".to_string()));
        assert!(tables.contains(&"namespace_objects".to_string()));
        assert!(tables.contains(&"namespace_audiences".to_string()));
        assert!(tables.contains(&"usage_events".to_string()));
        assert!(tables.contains(&"namespace_sessions".to_string()));
        assert!(tables.contains(&"custom_domains".to_string()));

        let user_cols: Vec<String> = conn
            .prepare("PRAGMA table_info(users)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(user_cols.contains(&"attachment_limit_bytes".to_string()));
        assert!(user_cols.contains(&"workspace_limit".to_string()));
        assert!(user_cols.contains(&"tier".to_string()));
        assert!(user_cols.contains(&"device_limit".to_string()));
        assert!(user_cols.contains(&"published_site_limit".to_string()));
        assert!(user_cols.contains(&"stripe_customer_id".to_string()));
        assert!(user_cols.contains(&"stripe_subscription_id".to_string()));
        assert!(user_cols.contains(&"apple_original_transaction_id".to_string()));
    }

    #[test]
    fn test_migrates_old_schema_and_adds_tier() {
        let conn = Connection::open_in_memory().unwrap();
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

        // Tier column should be added with default 'free'
        let tier: String = conn
            .query_row("SELECT tier FROM users WHERE id = 'u1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(tier, "free");

        // published_site_limit should be NULL
        let psl: Option<i64> = conn
            .query_row(
                "SELECT published_site_limit FROM users WHERE id = 'u1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(psl.is_none());
    }

    #[test]
    fn test_backfill_clears_old_200mib_default() {
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

        // u1 had the old 200MiB default → should be cleared to NULL
        let u1_limit: Option<i64> = conn
            .query_row(
                "SELECT attachment_limit_bytes FROM users WHERE id = 'u1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(u1_limit.is_none());

        // u2 had a custom limit → should be preserved
        let u2_limit: Option<i64> = conn
            .query_row(
                "SELECT attachment_limit_bytes FROM users WHERE id = 'u2'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(u2_limit, Some(500_000_000));
    }
}
