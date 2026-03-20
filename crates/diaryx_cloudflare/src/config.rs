//! Configuration from Worker environment variables / secrets.

use crate::adapters::resend::ResendMailer;
use diaryx_server::use_cases::auth::AuthConfig;
use worker::Env;

/// Read AuthConfig from environment, falling back to defaults.
pub fn auth_config(env: &Env) -> AuthConfig {
    AuthConfig {
        magic_link_expiry_minutes: env
            .var("MAGIC_LINK_EXPIRY_MINUTES")
            .ok()
            .and_then(|v| v.to_string().parse().ok())
            .unwrap_or(15),
        session_expiry_days: env
            .var("SESSION_EXPIRY_DAYS")
            .ok()
            .and_then(|v| v.to_string().parse().ok())
            .unwrap_or(30),
        rate_limit_per_hour: env
            .var("MAGIC_LINK_RATE_LIMIT")
            .ok()
            .and_then(|v| v.to_string().parse().ok())
            .unwrap_or(3),
    }
}

/// Read the app base URL for magic link URLs.
pub fn app_base_url(env: &Env) -> String {
    env.var("APP_BASE_URL")
        .map(|v| v.to_string())
        .unwrap_or_else(|_| "https://app.diaryx.org".to_string())
}

/// Read the HMAC signing key for audience tokens.
pub fn token_signing_key(env: &Env) -> Vec<u8> {
    env.secret("TOKEN_SIGNING_KEY")
        .map(|v| v.to_string().into_bytes())
        .unwrap_or_default()
}

/// Read session expiry in days (for Set-Cookie Max-Age).
pub fn session_expiry_days(env: &Env) -> i64 {
    env.var("SESSION_EXPIRY_DAYS")
        .ok()
        .and_then(|v| v.to_string().parse().ok())
        .unwrap_or(30)
}

/// Whether to set the Secure flag on cookies.
pub fn secure_cookies(env: &Env) -> bool {
    env.var("SECURE_COOKIES")
        .map(|v| v.to_string() != "false")
        .unwrap_or(true)
}

/// Build a ResendMailer if configured, or None for dev mode.
pub fn mailer(env: &Env, magic_link_expiry_minutes: i64) -> Option<ResendMailer> {
    let api_key = env.secret("RESEND_API_KEY").ok()?.to_string();
    if api_key.is_empty() {
        return None;
    }
    let from_name = env
        .var("EMAIL_FROM_NAME")
        .map(|v| v.to_string())
        .unwrap_or_else(|_| "Diaryx".to_string());
    let from_email = env
        .var("EMAIL_FROM_ADDRESS")
        .map(|v| v.to_string())
        .unwrap_or_else(|_| "noreply@diaryx.org".to_string());

    Some(ResendMailer::new(
        api_key,
        from_name,
        from_email,
        magic_link_expiry_minutes,
    ))
}

/// Read CORS allowed origins.
pub fn cors_origins(env: &Env) -> Vec<String> {
    env.var("CORS_ORIGINS")
        .map(|v| {
            v.to_string()
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_else(|_| vec!["https://app.diaryx.org".to_string()])
}
