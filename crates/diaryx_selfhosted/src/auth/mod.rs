mod magic_link;
mod middleware;
pub mod passkey;

pub use magic_link::{DeviceLimitDevice, MagicLinkError, MagicLinkService, VerifyResult};
pub use middleware::{
    AuthExtractor, AuthUser, OptionalAuth, RequireAuth, extract_token_from_query, validate_token,
};
pub use passkey::{PasskeyError, PasskeyInfo, PasskeyService};

/// Synchronous token validation for contexts that can't await (e.g. sync hooks).
/// This calls the repo directly — it will be removed when sync is ported to traits.
pub fn validate_token_sync(repo: &crate::db::AuthRepo, token: &str) -> Option<AuthUser> {
    let session = repo.validate_session(token).ok()??;
    let _ = repo.update_device_last_seen(&session.device_id);
    let user = repo.get_user(&session.user_id).ok()??;
    Some(AuthUser {
        session: diaryx_server::AuthSessionInfo {
            token: session.token,
            user_id: session.user_id,
            device_id: session.device_id,
            expires_at: session.expires_at,
            created_at: session.created_at,
        },
        user: diaryx_server::UserInfo {
            id: user.id,
            email: user.email,
            created_at: user.created_at,
            last_login_at: user.last_login_at,
            attachment_limit_bytes: user.attachment_limit_bytes,
            workspace_limit: user.workspace_limit,
            tier: match user.tier {
                crate::db::UserTier::Free => diaryx_server::UserTier::Free,
                crate::db::UserTier::Plus => diaryx_server::UserTier::Plus,
            },
            published_site_limit: user.published_site_limit,
        },
    })
}
