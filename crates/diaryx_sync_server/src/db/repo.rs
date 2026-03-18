use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use std::sync::{Arc, Mutex};

/// Tier-based limits for user accounts.
#[derive(Debug, Clone, Copy)]
pub struct TierDefaults {
    pub device_limit: u32,
    pub attachment_limit_bytes: u64,
    pub workspace_limit: u32,
    pub published_site_limit: u32,
}

/// User account tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserTier {
    Free,
    Plus,
}

impl UserTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            UserTier::Free => "free",
            UserTier::Plus => "plus",
        }
    }

    /// Parse a tier string; unknown values fall back to Free.
    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "plus" => UserTier::Plus,
            _ => UserTier::Free,
        }
    }

    pub fn defaults(&self) -> TierDefaults {
        match self {
            UserTier::Free => TierDefaults {
                device_limit: 2,
                attachment_limit_bytes: 200 * 1024 * 1024, // 200 MiB
                workspace_limit: 1,
                published_site_limit: 1,
            },
            UserTier::Plus => TierDefaults {
                device_limit: 10,
                attachment_limit_bytes: 2 * 1024 * 1024 * 1024, // 2 GiB
                workspace_limit: 10,
                published_site_limit: 1,
            },
        }
    }
}

/// User information
#[derive(Debug, Clone)]
pub struct UserInfo {
    pub id: String,
    pub email: String,
    pub created_at: DateTime<Utc>,
    pub last_login_at: Option<DateTime<Utc>>,
    pub attachment_limit_bytes: Option<u64>,
    pub workspace_limit: Option<u32>,
    pub tier: UserTier,
    pub published_site_limit: Option<u32>,
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

/// Passkey credential info for WebAuthn.
#[derive(Debug, Clone)]
pub struct PasskeyCredentialInfo {
    pub id: String,
    pub user_id: String,
    pub name: String,
    pub credential_json: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

/// Ephemeral passkey challenge state.
#[derive(Debug, Clone)]
pub struct PasskeyChallengeInfo {
    pub challenge_id: String,
    pub user_id: Option<String>,
    pub email: String,
    pub challenge_type: String,
    pub state_json: String,
    pub expires_at: i64,
    pub created_at: i64,
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

    /// Return a clone of the shared connection handle (for sharing with other repos).
    pub fn connection(&self) -> Arc<Mutex<Connection>> {
        self.conn.clone()
    }

    // ===== User operations =====

    /// Get a user by ID
    pub fn get_user(&self, user_id: &str) -> Result<Option<UserInfo>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, email, created_at, last_login_at, attachment_limit_bytes, workspace_limit, tier, published_site_limit FROM users WHERE id = ?",
            [user_id],
            |row| {
                Ok(UserInfo {
                    id: row.get(0)?,
                    email: row.get(1)?,
                    created_at: timestamp_to_datetime(row.get(2)?),
                    last_login_at: row.get::<_, Option<i64>>(3)?.map(timestamp_to_datetime),
                    attachment_limit_bytes: row.get::<_, Option<i64>>(4)?.map(|v| v as u64),
                    workspace_limit: row.get::<_, Option<i64>>(5)?.map(|v| v as u32),
                    tier: UserTier::from_str_lossy(&row.get::<_, String>(6)?),
                    published_site_limit: row.get::<_, Option<i64>>(7)?.map(|v| v as u32),
                })
            },
        )
        .optional()
    }

    /// Get a user by email
    pub fn get_user_by_email(&self, email: &str) -> Result<Option<UserInfo>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, email, created_at, last_login_at, attachment_limit_bytes, workspace_limit, tier, published_site_limit FROM users WHERE email = ?",
            [email],
            |row| {
                Ok(UserInfo {
                    id: row.get(0)?,
                    email: row.get(1)?,
                    created_at: timestamp_to_datetime(row.get(2)?),
                    last_login_at: row.get::<_, Option<i64>>(3)?.map(timestamp_to_datetime),
                    attachment_limit_bytes: row.get::<_, Option<i64>>(4)?.map(|v| v as u64),
                    workspace_limit: row.get::<_, Option<i64>>(5)?.map(|v| v as u32),
                    tier: UserTier::from_str_lossy(&row.get::<_, String>(6)?),
                    published_site_limit: row.get::<_, Option<i64>>(7)?.map(|v| v as u32),
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

        // Create new user (NULL limit columns → tier defaults apply)
        let user_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().timestamp();

        conn.execute(
            "INSERT INTO users (id, email, created_at, tier) VALUES (?, ?, ?, ?)",
            params![user_id, email, now, UserTier::Free.as_str()],
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

    /// Delete a user and all related data (devices, sessions, namespaces cascade).
    pub fn delete_user(&self, user_id: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM users WHERE id = ?", [user_id])?;
        Ok(())
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

    /// Count devices registered for a user.
    pub fn count_user_devices(&self, user_id: &str) -> Result<u32, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM devices WHERE user_id = ?",
            [user_id],
            |row| row.get(0),
        )?;
        Ok(count as u32)
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

    /// Rename a device. Returns true if the row was updated.
    pub fn rename_device(&self, device_id: &str, new_name: &str) -> Result<bool, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let updated = conn.execute(
            "UPDATE devices SET name = ? WHERE id = ?",
            params![new_name, device_id],
        )?;
        Ok(updated > 0)
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
    ) -> Result<(String, String), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();

        // Generate secure random token and 6-digit verification code
        let token = generate_secure_token();
        let code = generate_verification_code();
        let now = Utc::now().timestamp();

        conn.execute(
            "INSERT INTO magic_tokens (token, email, code, expires_at, created_at) VALUES (?, ?, ?, ?, ?)",
            params![token, email, code, expires_at.timestamp(), now],
        )?;

        Ok((token, code))
    }

    /// Verify and consume a magic token (returns email if valid)
    pub fn verify_magic_token(&self, token: &str) -> Result<Option<String>, rusqlite::Error> {
        let email = self.peek_magic_token(token)?;
        if email.is_some() {
            self.consume_magic_token(token)?;
        }
        Ok(email)
    }

    /// Check whether a magic-link token is valid without consuming it.
    pub fn peek_magic_token(&self, token: &str) -> Result<Option<String>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();

        conn.query_row(
            "SELECT email FROM magic_tokens WHERE token = ? AND used = 0 AND expires_at > ?",
            params![token, now],
            |row| row.get(0),
        )
        .optional()
    }

    /// Check whether a magic code is valid without consuming it. Returns the email on success.
    pub fn peek_magic_code(
        &self,
        code: &str,
        email: &str,
    ) -> Result<Option<String>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();

        let found: Option<String> = conn
            .query_row(
                "SELECT token FROM magic_tokens WHERE code = ? AND email = ? AND used = 0 AND expires_at > ?",
                params![code, email, now],
                |row| row.get(0),
            )
            .optional()?;

        Ok(found.map(|_| email.to_string()))
    }

    /// Mark a magic-link token (and its associated code) as consumed.
    pub fn consume_magic_token(&self, token: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE magic_tokens SET used = 1 WHERE token = ?", [token])?;
        Ok(())
    }

    /// Mark a magic code as consumed (by code + email lookup).
    pub fn consume_magic_code(&self, code: &str, email: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE magic_tokens SET used = 1 WHERE code = ? AND email = ?",
            params![code, email],
        )?;
        Ok(())
    }

    /// Verify and consume a magic code (returns email if valid).
    /// Looks up by (code, email) where used=0 and not expired, then marks used.
    pub fn verify_magic_code(
        &self,
        code: &str,
        email: &str,
    ) -> Result<Option<String>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();

        let result: Option<String> = conn
            .query_row(
                "SELECT token FROM magic_tokens WHERE code = ? AND email = ? AND used = 0 AND expires_at > ?",
                params![code, email, now],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(ref token) = result {
            // Mark the row as used (consumes both the link and the code)
            conn.execute(
                "UPDATE magic_tokens SET used = 1 WHERE token = ?",
                [token.as_str()],
            )?;
        }

        // Return the email on success (mirrors verify_magic_token API)
        Ok(result.map(|_| email.to_string()))
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

    // ===== Tier operations =====

    /// Get effective device limit for a user (per-user override wins, else tier default).
    pub fn get_effective_device_limit(&self, user_id: &str) -> Result<u32, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let row: Option<(String, Option<i64>)> = conn
            .query_row(
                "SELECT tier, device_limit FROM users WHERE id = ?",
                [user_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?)),
            )
            .optional()?;
        match row {
            Some((_tier_str, Some(limit))) => Ok(limit as u32),
            Some((tier_str, None)) => {
                Ok(UserTier::from_str_lossy(&tier_str).defaults().device_limit)
            }
            None => Ok(UserTier::Free.defaults().device_limit),
        }
    }

    /// Get the tier for a user.
    pub fn get_user_tier(&self, user_id: &str) -> Result<UserTier, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let tier_str: Option<String> = conn
            .query_row("SELECT tier FROM users WHERE id = ?", [user_id], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(tier_str
            .map(|s| UserTier::from_str_lossy(&s))
            .unwrap_or(UserTier::Free))
    }

    /// Set the tier for a user. Returns true if the row was updated.
    pub fn set_user_tier(&self, user_id: &str, tier: UserTier) -> Result<bool, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let updated = conn.execute(
            "UPDATE users SET tier = ? WHERE id = ?",
            params![tier.as_str(), user_id],
        )?;
        Ok(updated > 0)
    }

    // ===== Stripe billing operations =====

    /// Set the Stripe customer ID for a user.
    pub fn set_stripe_customer_id(
        &self,
        user_id: &str,
        customer_id: &str,
    ) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE users SET stripe_customer_id = ? WHERE id = ?",
            params![customer_id, user_id],
        )?;
        Ok(())
    }

    /// Get the Stripe customer ID for a user.
    pub fn get_stripe_customer_id(&self, user_id: &str) -> Result<Option<String>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let result: Option<Option<String>> = conn
            .query_row(
                "SELECT stripe_customer_id FROM users WHERE id = ?",
                [user_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(result.flatten())
    }

    /// Find a user ID by their Stripe customer ID.
    pub fn get_user_id_by_stripe_customer_id(
        &self,
        customer_id: &str,
    ) -> Result<Option<String>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id FROM users WHERE stripe_customer_id = ?",
            [customer_id],
            |row| row.get(0),
        )
        .optional()
    }

    /// Set the Stripe subscription ID for a user.
    pub fn set_stripe_subscription_id(
        &self,
        user_id: &str,
        subscription_id: Option<&str>,
    ) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE users SET stripe_subscription_id = ? WHERE id = ?",
            params![subscription_id, user_id],
        )?;
        Ok(())
    }

    // ===== Apple IAP operations =====

    /// Set the Apple original transaction ID for a user.
    pub fn set_apple_original_transaction_id(
        &self,
        user_id: &str,
        transaction_id: &str,
    ) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE users SET apple_original_transaction_id = ? WHERE id = ?",
            params![transaction_id, user_id],
        )?;
        Ok(())
    }

    /// Get the Apple original transaction ID for a user.
    pub fn get_apple_original_transaction_id(
        &self,
        user_id: &str,
    ) -> Result<Option<String>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let result: Option<Option<String>> = conn
            .query_row(
                "SELECT apple_original_transaction_id FROM users WHERE id = ?",
                [user_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(result.flatten())
    }

    /// Find a user ID by their Apple original transaction ID.
    pub fn get_user_id_by_apple_transaction_id(
        &self,
        transaction_id: &str,
    ) -> Result<Option<String>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id FROM users WHERE apple_original_transaction_id = ?",
            [transaction_id],
            |row| row.get(0),
        )
        .optional()
    }

    // ===== AI usage operations =====

    /// Get the managed AI request count for a user in a UTC month (`YYYY-MM`).
    pub fn get_user_ai_usage_monthly_count(
        &self,
        user_id: &str,
        period_utc: &str,
    ) -> Result<u64, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let count = conn
            .query_row(
                "SELECT request_count
                 FROM user_ai_usage_monthly
                 WHERE user_id = ? AND period_utc = ?",
                params![user_id, period_utc],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        Ok(count.unwrap_or(0).max(0) as u64)
    }

    /// Increment and return the managed AI usage counter for a user/month.
    pub fn increment_user_ai_usage_monthly_count(
        &self,
        user_id: &str,
        period_utc: &str,
    ) -> Result<u64, rusqlite::Error> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let now = Utc::now().timestamp();
        tx.execute(
            "INSERT INTO user_ai_usage_monthly (user_id, period_utc, request_count, updated_at)
             VALUES (?, ?, 1, ?)
             ON CONFLICT(user_id, period_utc) DO UPDATE SET
               request_count = request_count + 1,
               updated_at = excluded.updated_at",
            params![user_id, period_utc, now],
        )?;
        let count: i64 = tx.query_row(
            "SELECT request_count
             FROM user_ai_usage_monthly
             WHERE user_id = ? AND period_utc = ?",
            params![user_id, period_utc],
            |row| row.get(0),
        )?;
        tx.commit()?;
        Ok(count.max(0) as u64)
    }

    // ===== Passkey operations =====

    /// Store a passkey credential for a user.
    pub fn store_passkey_credential(
        &self,
        user_id: &str,
        name: &str,
        credential_json: &str,
    ) -> Result<String, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().timestamp();

        conn.execute(
            "INSERT INTO passkey_credentials (id, user_id, name, credential_json, created_at) VALUES (?, ?, ?, ?, ?)",
            params![id, user_id, name, credential_json, now],
        )?;

        Ok(id)
    }

    /// Get all passkey credentials for a user.
    pub fn get_passkey_credentials(
        &self,
        user_id: &str,
    ) -> Result<Vec<PasskeyCredentialInfo>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, user_id, name, credential_json, created_at, last_used_at \
             FROM passkey_credentials WHERE user_id = ? ORDER BY created_at DESC",
        )?;

        let rows = stmt
            .query_map([user_id], |row| {
                Ok(PasskeyCredentialInfo {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    name: row.get(2)?,
                    credential_json: row.get(3)?,
                    created_at: row.get(4)?,
                    last_used_at: row.get(5)?,
                })
            })?
            .filter_map(Result::ok)
            .collect();

        Ok(rows)
    }

    /// Get passkey credentials by email (looks up user first).
    pub fn get_passkey_credentials_by_email(
        &self,
        email: &str,
    ) -> Result<Vec<PasskeyCredentialInfo>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT pc.id, pc.user_id, pc.name, pc.credential_json, pc.created_at, pc.last_used_at \
             FROM passkey_credentials pc \
             JOIN users u ON u.id = pc.user_id \
             WHERE u.email = ? \
             ORDER BY pc.created_at DESC",
        )?;

        let rows = stmt
            .query_map([email], |row| {
                Ok(PasskeyCredentialInfo {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    name: row.get(2)?,
                    credential_json: row.get(3)?,
                    created_at: row.get(4)?,
                    last_used_at: row.get(5)?,
                })
            })?
            .filter_map(Result::ok)
            .collect();

        Ok(rows)
    }

    /// Update a passkey credential's last_used_at timestamp.
    pub fn update_passkey_credential_last_used(&self, id: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();
        conn.execute(
            "UPDATE passkey_credentials SET last_used_at = ? WHERE id = ?",
            params![now, id],
        )?;
        Ok(())
    }

    /// Update a passkey credential's JSON and last_used_at (after successful authentication).
    pub fn update_passkey_credential(
        &self,
        id: &str,
        credential_json: &str,
    ) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();
        conn.execute(
            "UPDATE passkey_credentials SET credential_json = ?, last_used_at = ? WHERE id = ?",
            params![credential_json, now, id],
        )?;
        Ok(())
    }

    /// Delete a passkey credential (owned by user_id).
    pub fn delete_passkey_credential(
        &self,
        id: &str,
        user_id: &str,
    ) -> Result<bool, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let deleted = conn.execute(
            "DELETE FROM passkey_credentials WHERE id = ? AND user_id = ?",
            params![id, user_id],
        )?;
        Ok(deleted > 0)
    }

    /// Store an ephemeral passkey challenge.
    pub fn store_passkey_challenge(
        &self,
        challenge_id: &str,
        user_id: Option<&str>,
        email: &str,
        challenge_type: &str,
        state_json: &str,
        expires_at: i64,
    ) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();
        conn.execute(
            "INSERT INTO passkey_challenges (challenge_id, user_id, email, challenge_type, state_json, expires_at, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![challenge_id, user_id, email, challenge_type, state_json, expires_at, now],
        )?;
        Ok(())
    }

    /// Retrieve and delete a passkey challenge (one-time use).
    pub fn get_passkey_challenge(
        &self,
        challenge_id: &str,
    ) -> Result<Option<PasskeyChallengeInfo>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();

        let result: Option<PasskeyChallengeInfo> = conn
            .query_row(
                "SELECT challenge_id, user_id, email, challenge_type, state_json, expires_at, created_at \
                 FROM passkey_challenges WHERE challenge_id = ? AND expires_at > ?",
                params![challenge_id, now],
                |row| {
                    Ok(PasskeyChallengeInfo {
                        challenge_id: row.get(0)?,
                        user_id: row.get(1)?,
                        email: row.get(2)?,
                        challenge_type: row.get(3)?,
                        state_json: row.get(4)?,
                        expires_at: row.get(5)?,
                        created_at: row.get(6)?,
                    })
                },
            )
            .optional()?;

        if result.is_some() {
            conn.execute(
                "DELETE FROM passkey_challenges WHERE challenge_id = ?",
                [challenge_id],
            )?;
        }

        Ok(result)
    }

    /// Clean up expired passkey challenges.
    pub fn cleanup_expired_passkey_challenges(&self) -> Result<usize, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();
        let deleted = conn.execute("DELETE FROM passkey_challenges WHERE expires_at < ?", [now])?;
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

/// Generate a 6-digit verification code (zero-padded)
fn generate_verification_code() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    format!("{:06}", rng.gen_range(0..1_000_000u32))
}

/// Convert Unix timestamp to DateTime<Utc>
fn timestamp_to_datetime(timestamp: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(timestamp, 0).unwrap_or_else(Utc::now)
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

        // Create token (now returns (token, code))
        let (token, code) = repo.create_magic_token(email, expires).unwrap();
        assert!(!token.is_empty());
        assert_eq!(code.len(), 6);

        // Verify token
        let verified_email = repo.verify_magic_token(&token).unwrap();
        assert_eq!(verified_email, Some(email.to_string()));

        // Token should be consumed (can't verify again)
        let second_verify = repo.verify_magic_token(&token).unwrap();
        assert!(second_verify.is_none());
    }

    #[test]
    fn test_magic_code_flow() {
        let repo = setup_test_db();
        let email = "code@example.com";
        let expires = Utc::now() + chrono::Duration::hours(1);

        let (_token, code) = repo.create_magic_token(email, expires).unwrap();

        // Verify by code + email
        let verified = repo.verify_magic_code(&code, email).unwrap();
        assert_eq!(verified, Some(email.to_string()));

        // Code should be consumed
        let second = repo.verify_magic_code(&code, email).unwrap();
        assert!(second.is_none());
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

        // Create user with device and session
        let user_id = repo.get_or_create_user("delete-test@example.com").unwrap();
        let device_id = repo
            .create_device(&user_id, Some("Test Device"), None)
            .unwrap();
        let expires = Utc::now() + chrono::Duration::days(30);
        let session_token = repo.create_session(&user_id, &device_id, expires).unwrap();

        // Verify everything exists
        assert!(repo.get_user(&user_id).unwrap().is_some());
        assert!(!repo.get_user_devices(&user_id).unwrap().is_empty());
        assert!(repo.validate_session(&session_token).unwrap().is_some());

        // Delete user
        repo.delete_user(&user_id).unwrap();

        // Verify everything is deleted (cascade)
        assert!(repo.get_user(&user_id).unwrap().is_none());
        assert!(repo.get_user_devices(&user_id).unwrap().is_empty());
        assert!(repo.validate_session(&session_token).unwrap().is_none());
    }

    #[test]
    fn test_user_tier_defaults_to_free() {
        let repo = setup_test_db();
        let user_id = repo.get_or_create_user("tier-test@example.com").unwrap();
        let user = repo.get_user(&user_id).unwrap().unwrap();
        assert_eq!(user.tier, UserTier::Free);

        let tier = repo.get_user_tier(&user_id).unwrap();
        assert_eq!(tier, UserTier::Free);
    }

    #[test]
    fn test_ai_usage_monthly_first_request_creates_and_increments() {
        let repo = setup_test_db();
        let user_id = repo
            .get_or_create_user("ai-usage-test@example.com")
            .unwrap();
        let period = "2026-03";

        assert_eq!(
            repo.get_user_ai_usage_monthly_count(&user_id, period)
                .unwrap(),
            0
        );
        assert_eq!(
            repo.increment_user_ai_usage_monthly_count(&user_id, period)
                .unwrap(),
            1
        );
        assert_eq!(
            repo.increment_user_ai_usage_monthly_count(&user_id, period)
                .unwrap(),
            2
        );
        assert_eq!(
            repo.get_user_ai_usage_monthly_count(&user_id, period)
                .unwrap(),
            2
        );
    }

    #[test]
    fn test_ai_usage_monthly_rollover_starts_new_counter() {
        let repo = setup_test_db();
        let user_id = repo
            .get_or_create_user("ai-usage-rollover@example.com")
            .unwrap();

        let march = "2026-03";
        let april = "2026-04";

        assert_eq!(
            repo.increment_user_ai_usage_monthly_count(&user_id, march)
                .unwrap(),
            1
        );
        assert_eq!(
            repo.get_user_ai_usage_monthly_count(&user_id, march)
                .unwrap(),
            1
        );
        assert_eq!(
            repo.get_user_ai_usage_monthly_count(&user_id, april)
                .unwrap(),
            0
        );
        assert_eq!(
            repo.increment_user_ai_usage_monthly_count(&user_id, april)
                .unwrap(),
            1
        );
    }
}
