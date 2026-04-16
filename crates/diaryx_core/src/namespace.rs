//! Platform-agnostic server-namespace management.
//!
//! Thin wrappers around the server's `/namespaces` endpoints that take any
//! [`AuthenticatedClient`] implementation, so CLI, Tauri, and Web all drive
//! the same code paths for namespace metadata and deletion instead of
//! open-coding the HTTP call in each platform.

use serde::{Deserialize, Serialize};

use crate::auth::{AuthError, AuthenticatedClient};

/// Server-side namespace metadata.
///
/// Mirrors the shape returned by `GET /namespaces/{id}`; only the fields
/// cross-platform callers actually need are modelled here (extra fields on
/// the wire are silently ignored by serde).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceMetadata {
    /// Server-assigned namespace id.
    pub id: String,
    /// User id that owns this namespace.
    pub owner_user_id: String,
    /// Unix-epoch seconds when the namespace was created.
    pub created_at: i64,
    /// Arbitrary metadata attached by the creator (e.g. `{ "name": "..." }`).
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

impl NamespaceMetadata {
    /// Best-effort display name from the `metadata.name` field.
    pub fn display_name(&self) -> Option<&str> {
        self.metadata
            .as_ref()
            .and_then(|m| m.get("name"))
            .and_then(|v| v.as_str())
    }
}

fn namespace_path(id: &str) -> String {
    format!("/namespaces/{}", urlencoding::encode(id))
}

/// Fetch metadata for a single namespace.
///
/// Returns an [`AuthError`] with the HTTP status on non-2xx responses, so
/// UI callers can distinguish 404 (already gone) from other failure modes.
pub async fn get_namespace<C: AuthenticatedClient>(
    client: &C,
    id: &str,
) -> Result<NamespaceMetadata, AuthError> {
    let resp = client.get(&namespace_path(id)).await?;
    if !resp.is_success() {
        return Err(AuthError::new(
            format!("Namespace lookup failed: HTTP {}", resp.status),
            resp.status,
        ));
    }
    resp.json()
}

/// Delete a namespace and all of its objects on the server.
///
/// This is a destructive, irreversible operation — any client currently
/// linked to the namespace will start getting 404s on its next sync and
/// will need to re-link. Returns `Ok(())` for 204 (deleted) and 404
/// (already gone, treated as idempotent), and an error otherwise.
pub async fn delete_namespace<C: AuthenticatedClient>(
    client: &C,
    id: &str,
) -> Result<(), AuthError> {
    let resp = client.delete(&namespace_path(id)).await?;
    match resp.status {
        204 => Ok(()),
        // 404 means another client (or a previous attempt) already deleted
        // it. From the caller's perspective the end state is identical to a
        // successful delete, so don't surface an error.
        404 => Ok(()),
        other => Err(AuthError::new(
            format!("Failed to delete namespace: HTTP {other}"),
            other,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{AuthMetadata, HttpResponse};
    use std::sync::Mutex;

    struct MockClient {
        responses: Mutex<Vec<HttpResponse>>,
        last_path: Mutex<Option<String>>,
    }

    impl MockClient {
        fn new(responses: Vec<HttpResponse>) -> Self {
            Self {
                responses: Mutex::new(responses),
                last_path: Mutex::new(None),
            }
        }

        fn record(&self, path: &str) {
            *self.last_path.lock().unwrap() = Some(path.to_string());
        }

        fn next_response(&self) -> Result<HttpResponse, AuthError> {
            let mut responses = self.responses.lock().unwrap();
            if responses.is_empty() {
                Err(AuthError::new("No mock response", 0))
            } else {
                Ok(responses.remove(0))
            }
        }
    }

    #[async_trait::async_trait]
    impl AuthenticatedClient for MockClient {
        fn server_url(&self) -> &str {
            "https://app.diaryx.org/api"
        }
        async fn has_session(&self) -> bool {
            true
        }
        async fn load_metadata(&self) -> Option<AuthMetadata> {
            None
        }
        async fn save_metadata(&self, _: &AuthMetadata) {}
        async fn store_session_token(&self, _: &str) {}
        async fn clear_session(&self) {}
        async fn get(&self, path: &str) -> Result<HttpResponse, AuthError> {
            self.record(path);
            self.next_response()
        }
        async fn post(&self, _: &str, _: Option<&str>) -> Result<HttpResponse, AuthError> {
            self.next_response()
        }
        async fn put(&self, _: &str, _: Option<&str>) -> Result<HttpResponse, AuthError> {
            self.next_response()
        }
        async fn patch(&self, _: &str, _: Option<&str>) -> Result<HttpResponse, AuthError> {
            self.next_response()
        }
        async fn delete(&self, path: &str) -> Result<HttpResponse, AuthError> {
            self.record(path);
            self.next_response()
        }
        async fn get_unauth(&self, _: &str) -> Result<HttpResponse, AuthError> {
            self.next_response()
        }
        async fn post_unauth(&self, _: &str, _: Option<&str>) -> Result<HttpResponse, AuthError> {
            self.next_response()
        }
    }

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        futures_lite::future::block_on(f)
    }

    #[test]
    fn delete_namespace_accepts_204() {
        let client = MockClient::new(vec![HttpResponse {
            status: 204,
            body: String::new(),
        }]);
        block_on(delete_namespace(&client, "ns-123")).expect("204 should be Ok");
        assert_eq!(
            client.last_path.lock().unwrap().as_deref(),
            Some("/namespaces/ns-123")
        );
    }

    #[test]
    fn delete_namespace_treats_404_as_idempotent() {
        // A 404 means someone else (or a retry of ours) already deleted
        // this namespace. The caller's intent ("make sure this is gone")
        // is satisfied, so we don't surface an error.
        let client = MockClient::new(vec![HttpResponse {
            status: 404,
            body: r#"{"error":"not found"}"#.to_string(),
        }]);
        block_on(delete_namespace(&client, "ns-123")).expect("404 should be Ok (idempotent)");
    }

    #[test]
    fn delete_namespace_surfaces_other_errors() {
        let client = MockClient::new(vec![HttpResponse {
            status: 500,
            body: String::new(),
        }]);
        let err = block_on(delete_namespace(&client, "ns-123"))
            .expect_err("500 should surface as an error");
        assert_eq!(err.status_code, 500);
    }

    #[test]
    fn delete_namespace_percent_encodes_ids() {
        // Ids that contain URL-reserved characters must round-trip
        // correctly to the server; the server looks up by the decoded
        // value so we encode on the way out.
        let client = MockClient::new(vec![HttpResponse {
            status: 204,
            body: String::new(),
        }]);
        block_on(delete_namespace(&client, "weird id/with spaces")).unwrap();
        assert_eq!(
            client.last_path.lock().unwrap().as_deref(),
            Some("/namespaces/weird%20id%2Fwith%20spaces")
        );
    }

    #[test]
    fn get_namespace_returns_metadata_on_success() {
        let client = MockClient::new(vec![HttpResponse {
            status: 200,
            body: r#"{
                "id": "ns-1",
                "owner_user_id": "user-1",
                "created_at": 1700000000,
                "metadata": { "name": "My Journal" }
            }"#
            .to_string(),
        }]);
        let ns = block_on(get_namespace(&client, "ns-1")).unwrap();
        assert_eq!(ns.id, "ns-1");
        assert_eq!(ns.display_name(), Some("My Journal"));
    }

    #[test]
    fn get_namespace_returns_err_on_non_2xx() {
        let client = MockClient::new(vec![HttpResponse {
            status: 404,
            body: r#"{"error":"not found"}"#.to_string(),
        }]);
        let err = block_on(get_namespace(&client, "ns-gone")).expect_err("404 must error");
        assert_eq!(err.status_code, 404);
    }
}
