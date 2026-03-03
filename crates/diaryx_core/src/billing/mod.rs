//! Billing tier model and feature gates.
//!
//! Provides a platform-agnostic [`BillingState`] with tier-based feature
//! gates. The billing state is queried from the sync server via
//! [`AuthHttpClient`](crate::auth::AuthHttpClient) (reusing the auth HTTP
//! layer) and can be refreshed by calling [`get_me()`](crate::auth::AuthService::get_me).
//!
//! Payment _initiation_ (Stripe redirect, Apple StoreKit purchase) stays
//! platform-specific. Core only models the resulting state.
//!
//! # Usage
//!
//! ```ignore
//! use diaryx_core::billing::{Tier, BillingState};
//! use diaryx_core::auth::MeResponse;
//!
//! let me: MeResponse = auth_service.get_me().await?;
//! let billing = BillingState::from_me_response(&me);
//!
//! assert!(billing.can_sync());
//! assert_eq!(billing.max_workspaces(), 10);
//! ```

use serde::{Deserialize, Serialize};

use crate::auth::MeResponse;

/// Subscription tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    /// Free tier — local-only, single workspace.
    Free,
    /// Plus tier — multi-device sync, collaboration, publishing.
    Plus,
}

impl Tier {
    /// Parse a tier from the server's string representation.
    pub fn from_str_loose(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "plus" => Tier::Plus,
            _ => Tier::Free,
        }
    }
}

impl std::fmt::Display for Tier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Tier::Free => write!(f, "free"),
            Tier::Plus => write!(f, "plus"),
        }
    }
}

/// Current billing state derived from the server's `/auth/me` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BillingState {
    /// Active subscription tier.
    pub tier: Tier,
    /// Maximum number of sync-enabled workspaces.
    pub workspace_limit: u32,
    /// Maximum attachment storage in bytes.
    pub storage_limit_bytes: u64,
    /// Maximum number of published sites.
    pub published_site_limit: u32,
}

// =========================================================================
// Free tier defaults
// =========================================================================

const FREE_WORKSPACE_LIMIT: u32 = 1;
const FREE_STORAGE_LIMIT_BYTES: u64 = 200 * 1024 * 1024; // 200 MB
const FREE_PUBLISHED_SITE_LIMIT: u32 = 1;

// =========================================================================
// Plus tier defaults (used as fallback when server doesn't specify)
// =========================================================================

const PLUS_WORKSPACE_LIMIT: u32 = 10;
const PLUS_STORAGE_LIMIT_BYTES: u64 = 2 * 1024 * 1024 * 1024; // 2 GiB
const PLUS_PUBLISHED_SITE_LIMIT: u32 = 5;

impl BillingState {
    /// Default free-tier billing state.
    pub fn free() -> Self {
        Self {
            tier: Tier::Free,
            workspace_limit: FREE_WORKSPACE_LIMIT,
            storage_limit_bytes: FREE_STORAGE_LIMIT_BYTES,
            published_site_limit: FREE_PUBLISHED_SITE_LIMIT,
        }
    }

    /// Default plus-tier billing state.
    pub fn plus() -> Self {
        Self {
            tier: Tier::Plus,
            workspace_limit: PLUS_WORKSPACE_LIMIT,
            storage_limit_bytes: PLUS_STORAGE_LIMIT_BYTES,
            published_site_limit: PLUS_PUBLISHED_SITE_LIMIT,
        }
    }

    /// Build billing state from a [`MeResponse`].
    ///
    /// The server provides exact limits; this method just maps them.
    pub fn from_me_response(me: &MeResponse) -> Self {
        let tier = Tier::from_str_loose(&me.tier);
        Self {
            tier,
            workspace_limit: me.workspace_limit,
            storage_limit_bytes: me.attachment_limit_bytes,
            published_site_limit: me.published_site_limit,
        }
    }

    // =====================================================================
    // Feature gates
    // =====================================================================

    /// Whether the user can enable multi-device sync.
    pub fn can_sync(&self) -> bool {
        self.tier == Tier::Plus
    }

    /// Whether the user can use live collaboration.
    pub fn can_collaborate(&self) -> bool {
        self.tier == Tier::Plus
    }

    /// Whether the user can publish sites.
    pub fn can_publish(&self) -> bool {
        self.tier == Tier::Plus
    }

    /// Maximum number of sync-enabled workspaces.
    pub fn max_workspaces(&self) -> u32 {
        self.workspace_limit
    }

    /// Maximum number of published sites.
    pub fn max_published_sites(&self) -> u32 {
        self.published_site_limit
    }

    /// Maximum attachment storage in bytes.
    pub fn max_storage_bytes(&self) -> u64 {
        self.storage_limit_bytes
    }

    /// Whether the user is on the Plus tier.
    pub fn is_plus(&self) -> bool {
        self.tier == Tier::Plus
    }
}

impl Default for BillingState {
    fn default() -> Self {
        Self::free()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_free_tier_defaults() {
        let state = BillingState::free();
        assert_eq!(state.tier, Tier::Free);
        assert!(!state.can_sync());
        assert!(!state.can_collaborate());
        assert!(!state.can_publish());
        assert_eq!(state.max_workspaces(), 1);
        assert_eq!(state.max_published_sites(), 1);
        assert_eq!(state.max_storage_bytes(), 200 * 1024 * 1024);
    }

    #[test]
    fn test_plus_tier_defaults() {
        let state = BillingState::plus();
        assert_eq!(state.tier, Tier::Plus);
        assert!(state.can_sync());
        assert!(state.can_collaborate());
        assert!(state.can_publish());
        assert_eq!(state.max_workspaces(), 10);
        assert_eq!(state.max_published_sites(), 5);
        assert_eq!(state.max_storage_bytes(), 2 * 1024 * 1024 * 1024);
    }

    #[test]
    fn test_from_me_response() {
        let me = MeResponse {
            user: crate::auth::User {
                id: "uid".to_string(),
                email: "u@e.com".to_string(),
            },
            workspaces: vec![],
            devices: vec![],
            workspace_limit: 10,
            tier: "plus".to_string(),
            published_site_limit: 5,
            attachment_limit_bytes: 2_147_483_648,
        };

        let state = BillingState::from_me_response(&me);
        assert_eq!(state.tier, Tier::Plus);
        assert_eq!(state.max_workspaces(), 10);
        assert!(state.can_sync());
    }

    #[test]
    fn test_tier_from_str_loose() {
        assert_eq!(Tier::from_str_loose("plus"), Tier::Plus);
        assert_eq!(Tier::from_str_loose("Plus"), Tier::Plus);
        assert_eq!(Tier::from_str_loose("PLUS"), Tier::Plus);
        assert_eq!(Tier::from_str_loose("free"), Tier::Free);
        assert_eq!(Tier::from_str_loose("unknown"), Tier::Free);
    }

    #[test]
    fn test_tier_display() {
        assert_eq!(Tier::Free.to_string(), "free");
        assert_eq!(Tier::Plus.to_string(), "plus");
    }

    #[test]
    fn test_default_is_free() {
        let state = BillingState::default();
        assert_eq!(state.tier, Tier::Free);
    }

    #[test]
    fn test_free_tier_me_response() {
        let me = MeResponse {
            user: crate::auth::User {
                id: "uid".to_string(),
                email: "u@e.com".to_string(),
            },
            workspaces: vec![],
            devices: vec![],
            workspace_limit: 1,
            tier: "free".to_string(),
            published_site_limit: 1,
            attachment_limit_bytes: 200 * 1024 * 1024,
        };

        let state = BillingState::from_me_response(&me);
        assert_eq!(state.tier, Tier::Free);
        assert!(!state.can_sync());
        assert_eq!(state.max_workspaces(), 1);
    }
}
