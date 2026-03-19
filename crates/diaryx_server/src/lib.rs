//! Platform-agnostic server core for Diaryx.
//!
//! This crate owns shared server-side domain models, semantic capability
//! traits, and portable use cases. Adapters are responsible for binding those
//! interfaces to concrete runtimes such as Axum/SQLite or Cloudflare Workers.

pub mod domain;
pub mod ports;
pub mod use_cases;

pub use domain::{
    AudienceInfo, CurrentUserContext, CustomDomainInfo, DeviceInfo, NamespaceInfo,
    NamespaceSessionInfo, TierDefaults, UserInfo, UserTier,
};
pub use ports::{
    AiProvider, AppleReceiptVerifier, AuthStore, BillingProvider, BlobStore, Clock,
    DomainMappingCache, JobSink, Mailer, MultipartCompletedPart, NamespaceStore, RateLimitStore,
    ServerCoreError, SessionStore, TokenClaims, TokenSigner,
};
