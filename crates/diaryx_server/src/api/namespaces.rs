//! Namespace API request/response types.

use crate::domain::NamespaceInfo;
use serde::{Deserialize, Serialize};

/// POST /namespaces
#[derive(Debug, Deserialize)]
pub struct CreateNamespaceRequest {
    /// Optional explicit ID (e.g. `"workspace:abc"`). If absent, a UUID is generated.
    pub id: Option<String>,
    /// Optional JSON metadata (e.g. `{"kind":"workspace","name":"My Journal"}`).
    pub metadata: Option<serde_json::Value>,
}

/// PATCH /namespaces/{id}
#[derive(Debug, Deserialize)]
pub struct UpdateNamespaceRequest {
    /// JSON metadata to set (or `null` to clear).
    pub metadata: Option<serde_json::Value>,
}

/// Namespace in API responses.
#[derive(Debug, Serialize, Deserialize)]
pub struct NamespaceResponse {
    pub id: String,
    pub owner_user_id: String,
    pub created_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

impl From<NamespaceInfo> for NamespaceResponse {
    fn from(ns: NamespaceInfo) -> Self {
        let metadata = ns
            .metadata
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok());
        Self {
            id: ns.id,
            owner_user_id: ns.owner_user_id,
            created_at: ns.created_at,
            metadata,
        }
    }
}

impl CreateNamespaceRequest {
    /// Extract metadata as a JSON string for the service layer.
    pub fn metadata_str(&self) -> Option<String> {
        self.metadata.as_ref().map(|v| v.to_string())
    }
}

impl UpdateNamespaceRequest {
    /// Extract metadata as a JSON string for the service layer.
    pub fn metadata_str(&self) -> Option<String> {
        self.metadata.as_ref().map(|v| v.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{CreateNamespaceRequest, NamespaceResponse, UpdateNamespaceRequest};
    use crate::domain::NamespaceInfo;

    #[test]
    fn namespace_response_parses_valid_metadata_json() {
        let response = NamespaceResponse::from(NamespaceInfo {
            id: "workspace:abc".to_string(),
            owner_user_id: "user1".to_string(),
            created_at: 123,
            metadata: Some(r#"{"name":"My Journal","kind":"workspace"}"#.to_string()),
        });

        assert_eq!(response.id, "workspace:abc");
        assert_eq!(response.owner_user_id, "user1");
        assert_eq!(
            response.metadata,
            Some(serde_json::json!({
                "name": "My Journal",
                "kind": "workspace"
            }))
        );
    }

    #[test]
    fn namespace_response_ignores_invalid_metadata_json() {
        let response = NamespaceResponse::from(NamespaceInfo {
            id: "workspace:abc".to_string(),
            owner_user_id: "user1".to_string(),
            created_at: 123,
            metadata: Some("{not-json}".to_string()),
        });

        assert_eq!(response.metadata, None);
    }

    #[test]
    fn metadata_helpers_serialize_request_bodies() {
        let create = CreateNamespaceRequest {
            id: Some("workspace:abc".to_string()),
            metadata: Some(serde_json::json!({
                "name": "My Journal",
                "archived": false
            })),
        };
        let update = UpdateNamespaceRequest {
            metadata: Some(serde_json::json!({ "theme": "paper" })),
        };

        assert_eq!(
            create.metadata_str().as_deref(),
            Some(r#"{"archived":false,"name":"My Journal"}"#)
        );
        assert_eq!(
            update.metadata_str().as_deref(),
            Some(r#"{"theme":"paper"}"#)
        );
    }
}
