mod magic_link;
mod middleware;
pub mod passkey;

pub use magic_link::{MagicLinkError, MagicLinkService, VerifyResult};
pub use middleware::{
    AuthExtractor, AuthUser, OptionalAuth, RequireAuth, extract_token_from_query, validate_token,
};
pub use passkey::{PasskeyError, PasskeyInfo, PasskeyService};
