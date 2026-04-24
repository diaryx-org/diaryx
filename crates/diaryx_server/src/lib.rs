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
pub mod audience_token;
pub mod domain;
pub mod ports;
pub mod proxy;
pub mod schema;
pub mod sync;
pub mod use_cases;

// The contract + testing helpers are native-only. Their trait impls assume
// `Send` futures (via plain `#[async_trait]`), which is incompatible with the
// `?Send` trait definitions `cfg_async_trait!` produces on `wasm32`. Since
// the Cloudflare worker never drives the contract suite from inside itself
// — `diaryx_cloudflare_e2e` does that from the native host — gating these
// off on wasm32 is the right call.
#[cfg(not(target_arch = "wasm32"))]
pub mod contract;
#[cfg(not(target_arch = "wasm32"))]
pub mod testing;

pub use domain::{
    AudienceInfo, AuthContext, AuthSessionInfo, CurrentUserContext, CustomDomainInfo, DeviceInfo,
    GateInput, GateRecord, NamespaceInfo, NamespaceSessionInfo, ObjectMeta, PasskeyChallengeInfo,
    PasskeyCredentialInfo, PasskeyInfo, PublicObjectAccess, TierDefaults, UsageTotals, UserInfo,
    UserTier,
};
pub use ports::{
    AppleReceiptVerifier, AuthSessionStore, AuthStore, BillingProvider, BillingStore, BlobStore,
    Clock, DeviceStore, DomainMappingCache, JobSink, MagicLinkStore, Mailer,
    MultipartCompletedPart, NamespaceStore, ObjectMetaStore, PasskeyStore, ProxyConfigStore,
    ProxySecretResolver, ProxyUsageStore, RateLimitStore, ServerCoreError, SessionStore,
    TokenClaims, TokenSigner, UserStore,
};
