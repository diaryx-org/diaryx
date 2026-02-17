use std::env;
use std::path::PathBuf;

/// Server configuration loaded from environment variables
#[derive(Debug, Clone)]
pub struct Config {
    /// Server host (default: 0.0.0.0)
    pub host: String,
    /// Server port (default: 3030)
    pub port: u16,
    /// Database file path (default: ./diaryx_sync.db)
    pub database_path: PathBuf,
    /// Base URL for magic link verification (e.g., https://app.diaryx.org)
    pub app_base_url: String,
    /// Email configuration
    pub email: EmailConfig,
    /// Session token expiration in days (default: 30)
    pub session_expiry_days: i64,
    /// Magic link token expiration in minutes (default: 15)
    pub magic_link_expiry_minutes: i64,
    /// CORS allowed origins (comma-separated)
    pub cors_origins: Vec<String>,
    /// Git auto-commit: minutes of inactivity before committing (default: 30)
    pub git_quiescence_minutes: u32,
    /// Git auto-commit: max hours before forcing a commit even with activity (default: 24)
    pub git_max_staleness_hours: u32,
    /// R2 blob storage configuration for attachment payloads
    pub r2: R2Config,
    /// Snapshot upload max size in bytes (default: 1 GiB)
    pub snapshot_upload_max_bytes: usize,
    /// Enable incremental attachment sync endpoints (default: true)
    pub attachment_incremental_sync_enabled: bool,
    /// R2 bucket for published static site artifacts
    pub sites_r2_bucket: String,
    /// Base URL for published sites (defaults to APP_BASE_URL)
    pub sites_base_url: String,
    /// Global HMAC key for audience access tokens (32 bytes)
    pub token_signing_key: Vec<u8>,
    /// Cloudflare KV namespace ID for domain→slug mapping
    pub kv_namespace_id: String,
    /// Cloudflare API token with KV write permissions
    pub kv_api_token: String,
    /// Optional admin secret for tier management endpoints
    pub admin_secret: Option<String>,
    /// Stripe billing configuration (None if STRIPE_SECRET_KEY not set)
    pub stripe: Option<StripeConfig>,
}

/// Stripe billing configuration.
#[derive(Debug, Clone)]
pub struct StripeConfig {
    /// Stripe secret key (sk_live_... or sk_test_...)
    pub secret_key: String,
    /// Webhook endpoint signing secret (whsec_...)
    pub webhook_secret: String,
    /// Price ID for the Plus plan (price_...)
    pub price_id: String,
    /// Publishable key returned to client (pk_live_... or pk_test_...)
    pub publishable_key: String,
}

/// Email configuration (Resend HTTP API)
#[derive(Debug, Clone)]
pub struct EmailConfig {
    /// Resend API key
    pub api_key: String,
    /// From email address
    pub from_email: String,
    /// From name (default: Diaryx)
    pub from_name: String,
}

/// R2 blob storage configuration.
#[derive(Debug, Clone)]
pub struct R2Config {
    /// Bucket name (default: diaryx-user-data)
    pub bucket: String,
    /// Cloudflare account ID
    pub account_id: String,
    /// Access key ID
    pub access_key_id: String,
    /// Secret access key
    pub secret_access_key: String,
    /// Optional endpoint override
    pub endpoint: Option<String>,
    /// Object key prefix (default: diaryx-sync)
    pub prefix: String,
    /// Retention days before physically deleting soft-deleted blobs (default: 7)
    pub gc_retention_days: i64,
}

impl Config {
    /// Load configuration from environment variables
    pub fn from_env() -> Result<Self, ConfigError> {
        dotenvy::dotenv().ok();

        let host = env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        let port = env::var("PORT")
            .unwrap_or_else(|_| "3030".to_string())
            .parse()
            .map_err(|_| ConfigError::InvalidPort)?;

        let database_path = PathBuf::from(
            env::var("DATABASE_PATH").unwrap_or_else(|_| "./diaryx_sync.db".to_string()),
        );

        let app_base_url =
            env::var("APP_BASE_URL").unwrap_or_else(|_| "http://localhost:5174".to_string());
        let sites_base_url = env::var("SITES_BASE_URL").unwrap_or_else(|_| app_base_url.clone());

        let email = EmailConfig {
            api_key: env::var("RESEND_API_KEY").unwrap_or_default(),
            from_email: env::var("EMAIL_FROM").unwrap_or_else(|_| "noreply@diaryx.org".to_string()),
            from_name: env::var("EMAIL_FROM_NAME").unwrap_or_else(|_| "Diaryx".to_string()),
        };

        let session_expiry_days = env::var("SESSION_EXPIRY_DAYS")
            .unwrap_or_else(|_| "30".to_string())
            .parse()
            .unwrap_or(30);

        let magic_link_expiry_minutes = env::var("MAGIC_LINK_EXPIRY_MINUTES")
            .unwrap_or_else(|_| "15".to_string())
            .parse()
            .unwrap_or(15);

        let cors_origins = env::var("CORS_ORIGINS")
            .unwrap_or_else(|_| "http://localhost:5174,http://localhost:5175".to_string())
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let git_quiescence_minutes = env::var("GIT_QUIESCENCE_MINUTES")
            .unwrap_or_else(|_| "30".to_string())
            .parse()
            .unwrap_or(30);

        let git_max_staleness_hours = env::var("GIT_MAX_STALENESS_HOURS")
            .unwrap_or_else(|_| "24".to_string())
            .parse()
            .unwrap_or(24);

        let r2 = R2Config {
            bucket: env::var("R2_BUCKET").unwrap_or_else(|_| "diaryx-user-data".to_string()),
            account_id: env::var("R2_ACCOUNT_ID").unwrap_or_default(),
            access_key_id: env::var("R2_ACCESS_KEY_ID").unwrap_or_default(),
            secret_access_key: env::var("R2_SECRET_ACCESS_KEY").unwrap_or_default(),
            endpoint: env::var("R2_ENDPOINT")
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty()),
            prefix: env::var("R2_PREFIX").unwrap_or_else(|_| "diaryx-sync".to_string()),
            gc_retention_days: env::var("R2_GC_RETENTION_DAYS")
                .unwrap_or_else(|_| "7".to_string())
                .parse()
                .unwrap_or(7),
        };

        let snapshot_upload_max_bytes = env::var("SNAPSHOT_UPLOAD_MAX_BYTES")
            .unwrap_or_else(|_| "1073741824".to_string())
            .parse()
            .unwrap_or(1073741824);
        let attachment_incremental_sync_enabled = env::var("ATTACHMENT_INCREMENTAL_SYNC_ENABLED")
            .unwrap_or_else(|_| "true".to_string())
            .eq_ignore_ascii_case("true");
        let sites_r2_bucket =
            env::var("SITES_R2_BUCKET").unwrap_or_else(|_| "diaryx-sites".to_string());

        let token_signing_key_raw = env::var("TOKEN_SIGNING_KEY")
            .unwrap_or_else(|_| "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_string());
        let token_signing_key = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            token_signing_key_raw.as_bytes(),
        )
        .or_else(|_| {
            base64::Engine::decode(
                &base64::engine::general_purpose::URL_SAFE_NO_PAD,
                token_signing_key_raw.as_bytes(),
            )
        })
        .map_err(|_| ConfigError::InvalidTokenSigningKey)?;
        if token_signing_key.len() != 32 {
            return Err(ConfigError::InvalidTokenSigningKey);
        }

        let kv_namespace_id = env::var("KV_NAMESPACE_ID").unwrap_or_default();
        let kv_api_token = env::var("KV_API_TOKEN").unwrap_or_default();
        let admin_secret = env::var("ADMIN_SECRET")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());

        let stripe = {
            let secret_key = env::var("STRIPE_SECRET_KEY").unwrap_or_default();
            if secret_key.is_empty() {
                None
            } else {
                Some(StripeConfig {
                    secret_key,
                    webhook_secret: env::var("STRIPE_WEBHOOK_SECRET").unwrap_or_default(),
                    price_id: env::var("STRIPE_PRICE_ID").unwrap_or_default(),
                    publishable_key: env::var("STRIPE_PUBLISHABLE_KEY").unwrap_or_default(),
                })
            }
        };

        Ok(Config {
            host,
            port,
            database_path,
            app_base_url,
            sites_base_url,
            email,
            session_expiry_days,
            magic_link_expiry_minutes,
            cors_origins,
            git_quiescence_minutes,
            git_max_staleness_hours,
            r2,
            snapshot_upload_max_bytes,
            attachment_incremental_sync_enabled,
            sites_r2_bucket,
            token_signing_key,
            kv_namespace_id,
            kv_api_token,
            admin_secret,
            stripe,
        })
    }

    /// Check if email sending is configured
    pub fn is_email_configured(&self) -> bool {
        !self.email.api_key.is_empty()
    }

    /// Get the server address
    pub fn server_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    /// Check if R2 credentials are configured.
    pub fn is_r2_configured(&self) -> bool {
        !self.r2.account_id.is_empty()
            && !self.r2.access_key_id.is_empty()
            && !self.r2.secret_access_key.is_empty()
    }

    /// Check if Stripe billing is configured.
    pub fn is_stripe_configured(&self) -> bool {
        self.stripe.is_some()
    }

    /// Check if Cloudflare KV is configured for custom domain mappings.
    pub fn is_kv_configured(&self) -> bool {
        !self.r2.account_id.is_empty()
            && !self.kv_namespace_id.is_empty()
            && !self.kv_api_token.is_empty()
    }
}

#[derive(Debug)]
pub enum ConfigError {
    InvalidPort,
    InvalidTokenSigningKey,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::InvalidPort => write!(f, "Invalid PORT environment variable"),
            ConfigError::InvalidTokenSigningKey => write!(
                f,
                "Invalid TOKEN_SIGNING_KEY (expected base64-encoded 32-byte key)"
            ),
        }
    }
}

impl std::error::Error for ConfigError {}
