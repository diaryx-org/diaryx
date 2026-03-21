//! Platform-agnostic server core for Diaryx.
//!
//! This crate owns shared server-side domain models, semantic capability
//! traits, and portable use cases. Adapters are responsible for binding those
//! interfaces to concrete runtimes such as Axum/SQLite or Cloudflare Workers.

/// Applies `#[async_trait]` with the correct `Send` bound for the target.
///
/// On native targets the futures must be `Send` (required by Axum / tokio).
/// On `wasm32` (Cloudflare Workers) the futures are `!Send` because the
/// runtime is single-threaded and JS-interop types are not `Send`.
#[macro_export]
macro_rules! cfg_async_trait {
    ($($item:item)*) => {
        $(
            #[cfg(not(target_arch = "wasm32"))]
            #[async_trait::async_trait]
            $item

            #[cfg(target_arch = "wasm32")]
            #[async_trait::async_trait(?Send)]
            $item
        )*
    };
}

pub mod api;
pub mod domain;
pub mod ports;
pub mod use_cases;

pub use domain::{
    AudienceInfo, AuthContext, AuthSessionInfo, CurrentUserContext, CustomDomainInfo, DeviceInfo,
    NamespaceInfo, NamespaceSessionInfo, ObjectMeta, PasskeyChallengeInfo, PasskeyCredentialInfo,
    PasskeyInfo, PublicObjectAccess, TierDefaults, UsageTotals, UserInfo, UserTier,
};
pub use ports::{
    AiProvider, AppleReceiptVerifier, AuthSessionStore, AuthStore, BillingProvider, BillingStore,
    BlobStore, Clock, DeviceStore, DomainMappingCache, JobSink, MagicLinkStore, Mailer,
    MultipartCompletedPart, NamespaceStore, ObjectMetaStore, PasskeyStore, RateLimitStore,
    ServerCoreError, SessionStore, TokenClaims, TokenSigner, UserStore,
};
