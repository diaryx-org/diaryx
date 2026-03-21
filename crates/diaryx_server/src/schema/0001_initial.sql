-- Initial schema for Diaryx server database (SQLite dialect).

-- Users
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

CREATE INDEX IF NOT EXISTS idx_users_stripe_customer
    ON users(stripe_customer_id) WHERE stripe_customer_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_users_apple_tx
    ON users(apple_original_transaction_id) WHERE apple_original_transaction_id IS NOT NULL;

-- Devices
CREATE TABLE IF NOT EXISTS devices (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT,
    user_agent TEXT,
    created_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_devices_user_id ON devices(user_id);

-- Magic link tokens
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
CREATE INDEX IF NOT EXISTS idx_magic_tokens_code ON magic_tokens(code);

-- Auth sessions
CREATE TABLE IF NOT EXISTS auth_sessions (
    token TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    device_id TEXT NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    expires_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_sessions_user_id ON auth_sessions(user_id);
CREATE INDEX IF NOT EXISTS idx_sessions_expires ON auth_sessions(expires_at);

-- AI usage counters
CREATE TABLE IF NOT EXISTS user_ai_usage_monthly (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    period_utc TEXT NOT NULL,
    request_count INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, period_utc)
);

CREATE INDEX IF NOT EXISTS idx_user_ai_usage_monthly_user ON user_ai_usage_monthly(user_id);

-- Namespaces
CREATE TABLE IF NOT EXISTS namespaces (
    id TEXT PRIMARY KEY,
    owner_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_namespaces_owner ON namespaces(owner_user_id);

-- Namespace objects
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

-- Namespace audiences
CREATE TABLE IF NOT EXISTS namespace_audiences (
    namespace_id TEXT NOT NULL REFERENCES namespaces(id) ON DELETE CASCADE,
    audience_name TEXT NOT NULL,
    access TEXT NOT NULL DEFAULT 'private',
    PRIMARY KEY (namespace_id, audience_name)
);

CREATE INDEX IF NOT EXISTS idx_namespace_audiences_ns ON namespace_audiences(namespace_id);

-- Custom domains
CREATE TABLE IF NOT EXISTS custom_domains (
    domain TEXT PRIMARY KEY,
    namespace_id TEXT NOT NULL REFERENCES namespaces(id) ON DELETE CASCADE,
    audience_name TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    verified INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_custom_domains_ns ON custom_domains(namespace_id);

-- Usage events
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

-- Namespace sessions
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

-- Passkey credentials
CREATE TABLE IF NOT EXISTS passkey_credentials (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    credential_json TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    last_used_at INTEGER
);

CREATE INDEX IF NOT EXISTS idx_passkey_credentials_user ON passkey_credentials(user_id);

-- Passkey challenges (ephemeral)
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
