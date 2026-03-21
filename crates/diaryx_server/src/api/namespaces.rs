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
