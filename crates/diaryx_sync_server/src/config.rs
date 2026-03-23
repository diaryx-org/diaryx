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
    /// R2 blob storage configuration
    pub r2: R2Config,
    /// Global HMAC key for audience access tokens (32 bytes)
    pub token_signing_key: Vec<u8>,
    /// Optional admin secret for tier management endpoints
    pub admin_secret: Option<String>,
    /// Managed AI proxy configuration.
    pub managed_ai: ManagedAiConfig,
    /// Stripe billing configuration (None if STRIPE_SECRET_KEY not set)
    pub stripe: Option<StripeConfig>,
    /// Apple IAP configuration (None if APPLE_IAP_BUNDLE_ID not set)
    pub apple_iap: Option<AppleIapConfig>,
    /// Whether to set the `Secure` flag on session cookies.
    /// Derived from `app_base_url`: true when it starts with `https://`.
    pub secure_cookies: bool,
    /// Local filesystem path for blob storage (default: sibling of DATABASE_PATH)
    pub blob_store_path: std::path::PathBuf,
    /// Use volatile in-memory blob store instead of filesystem (BLOB_STORE_IN_MEMORY=1)
    pub blob_store_in_memory: bool,
    /// Cloudflare KV API token for writing domain mappings
    pub kv_api_token: Option<String>,
    /// Cloudflare KV namespace ID for domain mappings
    pub kv_namespace_id: Option<String>,
    /// Public URL for serving published sites. Defaults to `http://{host}:{port}`.
    /// Override with `SITE_BASE_URL` for tunnels or load balancers.
    pub site_base_url: String,
    /// Domain for subdomain-based site serving (e.g., "diaryx.org" or "notes.example.com").
    /// When set, subdomains are available (requires DNS wildcard + reverse proxy).
    /// When empty, sites are served at `/sites/{ns_id}/` paths only.
    pub site_domain: Option<String>,
}

/// Managed AI proxy configuration.
#[derive(Debug, Clone)]
pub struct ManagedAiConfig {
    /// OpenRouter API key used by server-side managed proxy calls.
    pub openrouter_api_key: String,
    /// OpenRouter chat completions endpoint.
    pub openrouter_endpoint: String,
    /// Allowlisted managed models.
    pub models: Vec<String>,
    /// Per-user requests per minute.
    pub rate_limit_per_minute: usize,
    /// Per-user request quota per UTC calendar month.
    pub monthly_quota: u64,
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

/// Apple IAP configuration.
#[derive(Debug, Clone)]
pub struct AppleIapConfig {
    /// App bundle ID (e.g., org.diaryx.desktop)
    pub bundle_id: String,
    /// Environment: "Sandbox" or "Production"
    pub environment: String,
    /// Skip JWS signature verification (for local StoreKit testing only)
    pub skip_signature_verify: bool,
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
        };

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

        let admin_secret = env::var("ADMIN_SECRET")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());

        let managed_ai_models = env::var("MANAGED_AI_MODELS")
            .ok()
            .map(|raw| {
                raw.split(',')
                    .map(|model| model.trim().to_string())
                    .filter(|model| !model.is_empty())
                    .collect::<Vec<_>>()
            })
            .filter(|models| !models.is_empty())
            .unwrap_or_else(|| {
                vec![
                    "google/gemini-3-flash-preview".to_string(),
                    "anthropic/claude-haiku-4.5".to_string(),
                    "openai/gpt-5.2".to_string(),
                ]
            });

        let managed_ai = ManagedAiConfig {
            openrouter_api_key: env::var("MANAGED_AI_OPENROUTER_API_KEY").unwrap_or_default(),
            openrouter_endpoint: env::var("MANAGED_AI_OPENROUTER_ENDPOINT")
                .unwrap_or_else(|_| "https://openrouter.ai/api/v1/chat/completions".to_string()),
            models: managed_ai_models,
            rate_limit_per_minute: env::var("MANAGED_AI_RATE_LIMIT_PER_MINUTE")
                .unwrap_or_else(|_| "30".to_string())
                .parse()
                .unwrap_or(30),
            monthly_quota: env::var("MANAGED_AI_MONTHLY_QUOTA")
                .unwrap_or_else(|_| "1000".to_string())
                .parse()
                .unwrap_or(1000),
        };

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

        let apple_iap = {
            let bundle_id = env::var("APPLE_IAP_BUNDLE_ID").unwrap_or_default();
            if bundle_id.is_empty() {
                None
            } else {
                let skip_sig = env::var("APPLE_IAP_SKIP_SIGNATURE_VERIFY").unwrap_or_default();
                Some(AppleIapConfig {
                    bundle_id,
                    environment: env::var("APPLE_IAP_ENVIRONMENT")
                        .unwrap_or_else(|_| "Sandbox".to_string()),
                    skip_signature_verify: skip_sig == "true" || skip_sig == "1",
                })
            }
        };

        let blob_store_path = env::var("BLOB_STORE_PATH")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                database_path
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new("."))
                    .join("blobs")
            });

        let blob_store_in_memory = env::var("BLOB_STORE_IN_MEMORY")
            .ok()
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        let kv_api_token = env::var("KV_API_TOKEN")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        let kv_namespace_id = env::var("KV_NAMESPACE_ID")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());

        let secure_cookies = app_base_url.starts_with("https://");

        let site_base_url = env::var("SITE_BASE_URL")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| {
                // 0.0.0.0 is a bind address, not browsable — use localhost instead
                let display_host = if host == "0.0.0.0" || host == "::" {
                    "localhost"
                } else {
                    &host
                };
                format!("http://{}:{}", display_host, port)
            });

        // SITE_DOMAIN enables subdomain/custom-domain features.
        // Auto-detected from R2+KV (Cloudflare deployment), or set explicitly.
        let site_domain = env::var("SITE_DOMAIN")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());

        Ok(Config {
            host,
            port,
            database_path,
            app_base_url,
            email,
            session_expiry_days,
            magic_link_expiry_minutes,
            cors_origins,
            r2,
            token_signing_key,
            secure_cookies,
            blob_store_path,
            blob_store_in_memory,
            admin_secret,
            managed_ai,
            stripe,
            apple_iap,
            kv_api_token,
            kv_namespace_id,
            site_base_url,
            site_domain,
        })
    }

    /// Check if email sending is configured
    pub fn is_email_configured(&self) -> bool {
        !self.email.api_key.is_empty()
    }

    /// Whether subdomain/custom-domain features are available.
    pub fn subdomains_available(&self) -> bool {
        self.site_domain.is_some()
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

    /// Check if Apple IAP is configured.
    pub fn is_apple_iap_configured(&self) -> bool {
        self.apple_iap.is_some()
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
