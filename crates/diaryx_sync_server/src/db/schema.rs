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
    published_site_limit INTEGER,
    stripe_customer_id TEXT,
    stripe_subscription_id TEXT
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

-- User workspaces (links users to their workspace CRDTs)
CREATE TABLE IF NOT EXISTS user_workspaces (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL DEFAULT 'default',
    created_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_user_workspace_name ON user_workspaces(user_id, name);

-- Share sessions (for live collaboration)
CREATE TABLE IF NOT EXISTS share_sessions (
    code TEXT PRIMARY KEY,              -- XXXX-XXXX format
    workspace_id TEXT NOT NULL,
    owner_user_id TEXT NOT NULL,
    read_only INTEGER DEFAULT 0,
    created_at INTEGER NOT NULL,
    expires_at INTEGER,                 -- NULL = no expiry
    FOREIGN KEY (owner_user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_share_sessions_owner ON share_sessions(owner_user_id);
CREATE INDEX IF NOT EXISTS idx_share_sessions_workspace ON share_sessions(workspace_id);

-- Per-user deduplicated attachment blobs stored in R2.
CREATE TABLE IF NOT EXISTS user_attachment_blobs (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    blob_hash TEXT NOT NULL,
    r2_key TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    mime_type TEXT NOT NULL,
    ref_count INTEGER NOT NULL DEFAULT 0,
    soft_deleted_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, blob_hash)
);

CREATE INDEX IF NOT EXISTS idx_user_attachment_blobs_user ON user_attachment_blobs(user_id);
CREATE INDEX IF NOT EXISTS idx_user_attachment_blobs_soft_delete ON user_attachment_blobs(soft_deleted_at);

-- Workspace attachment references (path -> blob hash mapping).
CREATE TABLE IF NOT EXISTS workspace_attachment_refs (
    workspace_id TEXT NOT NULL REFERENCES user_workspaces(id) ON DELETE CASCADE,
    file_path TEXT NOT NULL,
    attachment_path TEXT NOT NULL,
    blob_hash TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    mime_type TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, file_path, attachment_path)
);

CREATE INDEX IF NOT EXISTS idx_workspace_attachment_refs_workspace ON workspace_attachment_refs(workspace_id);

-- Attachment multipart upload sessions (resumable server-proxy uploads).
CREATE TABLE IF NOT EXISTS attachment_uploads (
    upload_id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES user_workspaces(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    blob_hash TEXT NOT NULL,
    attachment_path TEXT NOT NULL,
    mime_type TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    part_size INTEGER NOT NULL,
    total_parts INTEGER NOT NULL,
    r2_key TEXT NOT NULL,
    r2_multipart_upload_id TEXT NOT NULL,
    status TEXT NOT NULL, -- uploading|completed|aborted|expired
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_attachment_uploads_workspace ON attachment_uploads(workspace_id);
CREATE INDEX IF NOT EXISTS idx_attachment_uploads_user ON attachment_uploads(user_id);
CREATE INDEX IF NOT EXISTS idx_attachment_uploads_expires ON attachment_uploads(expires_at);

CREATE TABLE IF NOT EXISTS attachment_upload_parts (
    upload_id TEXT NOT NULL REFERENCES attachment_uploads(upload_id) ON DELETE CASCADE,
    part_no INTEGER NOT NULL,
    etag TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (upload_id, part_no)
);

CREATE INDEX IF NOT EXISTS idx_attachment_upload_parts_upload ON attachment_upload_parts(upload_id);

-- Published static sites.
CREATE TABLE IF NOT EXISTS published_sites (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES user_workspaces(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    slug TEXT NOT NULL UNIQUE,
    custom_domain TEXT,
    enabled INTEGER NOT NULL DEFAULT 1,
    auto_publish INTEGER NOT NULL DEFAULT 1,
    last_published_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_published_sites_workspace ON published_sites(workspace_id);
CREATE INDEX IF NOT EXISTS idx_published_sites_user ON published_sites(user_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_published_sites_custom_domain
  ON published_sites(custom_domain) WHERE custom_domain IS NOT NULL;

CREATE TABLE IF NOT EXISTS site_audience_builds (
    site_id TEXT NOT NULL REFERENCES published_sites(id) ON DELETE CASCADE,
    audience TEXT NOT NULL,
    file_count INTEGER NOT NULL DEFAULT 0,
    built_at INTEGER NOT NULL,
    PRIMARY KEY (site_id, audience)
);

CREATE TABLE IF NOT EXISTS site_access_tokens (
    id TEXT PRIMARY KEY,
    site_id TEXT NOT NULL REFERENCES published_sites(id) ON DELETE CASCADE,
    audience TEXT NOT NULL,
    label TEXT,
    expires_at INTEGER,
    revoked INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_site_access_tokens_site ON site_access_tokens(site_id);
"#;

const PUBLISHED_SITE_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS published_sites (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES user_workspaces(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    slug TEXT NOT NULL UNIQUE,
    enabled INTEGER NOT NULL DEFAULT 1,
    auto_publish INTEGER NOT NULL DEFAULT 1,
    last_published_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_published_sites_workspace ON published_sites(workspace_id);
CREATE INDEX IF NOT EXISTS idx_published_sites_user ON published_sites(user_id);

CREATE TABLE IF NOT EXISTS site_audience_builds (
    site_id TEXT NOT NULL REFERENCES published_sites(id) ON DELETE CASCADE,
    audience TEXT NOT NULL,
    file_count INTEGER NOT NULL DEFAULT 0,
    built_at INTEGER NOT NULL,
    PRIMARY KEY (site_id, audience)
);

CREATE TABLE IF NOT EXISTS site_access_tokens (
    id TEXT PRIMARY KEY,
    site_id TEXT NOT NULL REFERENCES published_sites(id) ON DELETE CASCADE,
    audience TEXT NOT NULL,
    label TEXT,
    expires_at INTEGER,
    revoked INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_site_access_tokens_site ON site_access_tokens(site_id);
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

    // Backfill: clear explicit attachment limits that match the old Free default (200 MiB)
    // so tier defaults take effect cleanly.
    conn.execute(
        "UPDATE users SET attachment_limit_bytes = NULL WHERE attachment_limit_bytes = 209715200",
        [],
    )?;

    let has_published_sites = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='published_sites' LIMIT 1",
            [],
            |_| Ok(()),
        )
        .is_ok();
    if !has_published_sites {
        conn.execute_batch(PUBLISHED_SITE_SCHEMA)?;
    }

    // Forward migration: add custom_domain column to published_sites.
    let has_custom_domain_col: bool = conn
        .prepare("PRAGMA table_info(published_sites)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(Result::ok)
        .any(|name| name == "custom_domain");
    if !has_custom_domain_col {
        conn.execute(
            "ALTER TABLE published_sites ADD COLUMN custom_domain TEXT",
            [],
        )?;
        conn.execute_batch(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_published_sites_custom_domain \
             ON published_sites(custom_domain) WHERE custom_domain IS NOT NULL;",
        )?;
    }

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
        assert!(tables.contains(&"user_workspaces".to_string()));
        assert!(tables.contains(&"share_sessions".to_string()));
        assert!(tables.contains(&"user_attachment_blobs".to_string()));
        assert!(tables.contains(&"workspace_attachment_refs".to_string()));
        assert!(tables.contains(&"attachment_uploads".to_string()));
        assert!(tables.contains(&"attachment_upload_parts".to_string()));
        assert!(tables.contains(&"published_sites".to_string()));
        assert!(tables.contains(&"site_audience_builds".to_string()));
        assert!(tables.contains(&"site_access_tokens".to_string()));

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
        assert!(user_cols.contains(&"published_site_limit".to_string()));
        assert!(user_cols.contains(&"stripe_customer_id".to_string()));
        assert!(user_cols.contains(&"stripe_subscription_id".to_string()));

        let site_cols: Vec<String> = conn
            .prepare("PRAGMA table_info(published_sites)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(site_cols.contains(&"custom_domain".to_string()));
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
