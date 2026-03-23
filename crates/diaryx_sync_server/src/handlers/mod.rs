pub mod ai;
pub mod apple;
pub mod audiences;
pub mod auth;
pub mod domains;
pub mod namespaces;
pub mod ns_sessions;
pub mod objects;
pub mod proxy;
pub mod sites;
pub mod stripe;
pub mod subscribers;

pub use ai::ai_routes;
pub use apple::apple_iap_routes;
pub use audiences::{AudienceState, audience_routes};
pub use auth::auth_routes;
pub use domains::{DomainState, domain_auth_route, domain_routes};
pub use namespaces::{NamespaceState, namespace_routes};
pub use ns_sessions::{NsSessionState, ns_session_routes};
pub use objects::{ObjectState, object_routes, public_object_routes, usage_routes};
pub use proxy::{ProxyState, proxy_routes};
pub use sites::site_routes;
pub use stripe::stripe_routes;
pub use subscribers::{SubscriberState, subscriber_routes};

use crate::db::NamespaceRepo;
use axum::http::StatusCode;
use axum::response::IntoResponse;

/// Verify the caller owns the namespace, returning an error response if not.
pub(crate) fn require_namespace_owner(
    ns_repo: &NamespaceRepo,
    namespace_id: &str,
    caller_user_id: &str,
) -> Result<(), axum::response::Response> {
    match ns_repo.get_namespace(namespace_id) {
        Some(ns) if ns.owner_user_id == caller_user_id => Ok(()),
        Some(_) => Err(StatusCode::FORBIDDEN.into_response()),
        None => Err(StatusCode::NOT_FOUND.into_response()),
    }
}
