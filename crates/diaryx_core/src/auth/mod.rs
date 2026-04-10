//! Authentication module for the Diaryx sync server.
//!
//! Provides a platform-agnostic [`AuthService`] that handles magic link
//! authentication, session management, and user info queries. Platform-specific
//! concerns — HTTP transport and credential storage — are bundled into a single
//! [`AuthenticatedClient`] trait so that the session token never appears in
//! service-level code.
//!
//! # Architecture
//!
//! ```text
//! ┌──────────────────────┐
//! │      AuthService     │ ← platform-agnostic business logic
//! ├──────────────────────┤
//! │  AuthenticatedClient │ ← bundles HTTP + credential storage per platform:
//! │                      │   • CLI: FsAuthenticatedClient (auth.md + ureq)
//! │                      │   • Tauri: KeyringAuthenticatedClient (OS keyring)
//! │                      │   • Web: BrowserAuthenticatedClient (HttpOnly cookie)
//! └──────────────────────┘
//! ```
//!
//! # Usage
//!
//! ```ignore
//! use diaryx_core::auth::AuthService;
//!
//! let client = /* platform-specific AuthenticatedClient */;
//! let service = AuthService::new(client);
//! service.request_magic_link("user@example.com").await?;
//! let verify = service.verify_magic_link("token123", Some("CLI")).await?;
//! ```

mod service;
mod types;

pub use service::AuthService;
pub use types::*;
