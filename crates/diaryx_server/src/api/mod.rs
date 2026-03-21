//! Shared API request/response types.
//!
//! These DTOs are the canonical wire format for the Diaryx server API.
//! Both the Axum (native) and Cloudflare Workers handler layers import
//! these types, ensuring the two implementations expose identical schemas.

pub mod billing;
pub mod namespaces;
