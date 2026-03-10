//! Authentication module for Diaryx sync server.
//!
//! Provides a platform-agnostic [`AuthService`] that handles magic link
//! authentication, session management, and user info queries. Platform-specific
//! HTTP and storage are injected via the [`AuthHttpClient`] and [`AuthStorage`]
//! traits.
//!
//! # Architecture
//!
//! ```text
//! ┌───────────────┐
//! │  AuthService   │ ← platform-agnostic business logic
//! ├───────────────┤
//! │ AuthHttpClient │ ← trait: reqwest (CLI), fetch (WASM)
//! │ AuthStorage    │ ← trait: Config TOML (CLI), localStorage (WASM)
//! └───────────────┘
//! ```
//!
//! # Usage
//!
//! ```ignore
//! use diaryx_core::auth::{AuthService, AuthCredentials};
//!
//! let service = AuthService::new(http_client, storage);
//! service.request_magic_link("user@example.com").await?;
//! let creds = service.verify_magic_link("token123", Some("CLI")).await?;
//! ```

#[cfg(all(not(target_arch = "wasm32"), feature = "toml-config"))]
mod native_storage;
mod service;
mod types;

#[cfg(all(not(target_arch = "wasm32"), feature = "toml-config"))]
pub use native_storage::NativeFileAuthStorage;
pub use service::AuthService;
pub use types::*;
