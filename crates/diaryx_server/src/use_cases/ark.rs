//! ARK identity registration and resolution.
//!
//! Registration happens at publish time, riding on the owner-authenticated
//! object PUT — so ownership is already enforced by the caller before this
//! service runs. The service's job is the collision check and the upsert.

use crate::domain::ArkIndexEntry;
use crate::ports::{ArkIndexStore, ServerCoreError};

pub struct ArkService<'a> {
    ark_index: &'a dyn ArkIndexStore,
}

impl<'a> ArkService<'a> {
    pub fn new(ark_index: &'a dyn ArkIndexStore) -> Self {
        Self { ark_index }
    }

    /// Register the object key a file ARK resolves to within a workspace.
    ///
    /// Rejects with [`ServerCoreError::Conflict`] when the file ARK is already
    /// registered to a *different* object — a cross-device collision on a
    /// still-provisional id — so the client remints and republishes. Re-registering
    /// the same object (a republish) is idempotent.
    pub async fn register(
        &self,
        workspace_ark: &str,
        file_ark: &str,
        object_key: &str,
        audience: Option<&str>,
    ) -> Result<(), ServerCoreError> {
        if let Some(existing_key) = self
            .ark_index
            .get_ark_owner(workspace_ark, file_ark)
            .await?
            && existing_key != object_key
        {
            return Err(ServerCoreError::conflict(
                "File ARK already registered to a different object",
            ));
        }

        self.ark_index
            .upsert_ark(workspace_ark, file_ark, object_key, audience)
            .await
    }

    /// Resolve a file ARK within a workspace to its current index entry.
    pub async fn resolve(
        &self,
        workspace_ark: &str,
        file_ark: &str,
    ) -> Result<ArkIndexEntry, ServerCoreError> {
        self.ark_index
            .resolve_ark(workspace_ark, file_ark)
            .await?
            .ok_or_else(|| ServerCoreError::not_found("ARK not found"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// `(workspace_ark, file_ark)` → `(object_key, audience)`.
    type ArkRows = HashMap<(String, String), (String, Option<String>)>;

    #[derive(Default)]
    struct TestArkIndexStore {
        rows: Mutex<ArkRows>,
    }

    crate::cfg_async_trait! {
    impl ArkIndexStore for TestArkIndexStore {
        async fn upsert_ark(
            &self,
            workspace_ark: &str,
            file_ark: &str,
            object_key: &str,
            audience: Option<&str>,
        ) -> Result<(), ServerCoreError> {
            self.rows.lock().unwrap().insert(
                (workspace_ark.to_string(), file_ark.to_string()),
                (object_key.to_string(), audience.map(String::from)),
            );
            Ok(())
        }
        async fn resolve_ark(
            &self,
            workspace_ark: &str,
            file_ark: &str,
        ) -> Result<Option<ArkIndexEntry>, ServerCoreError> {
            Ok(self
                .rows
                .lock()
                .unwrap()
                .get(&(workspace_ark.to_string(), file_ark.to_string()))
                .map(|(object_key, audience)| ArkIndexEntry {
                    workspace_ark: workspace_ark.to_string(),
                    file_ark: file_ark.to_string(),
                    object_key: object_key.clone(),
                    audience: audience.clone(),
                    updated_at: 1,
                }))
        }
        async fn get_ark_owner(
            &self,
            workspace_ark: &str,
            file_ark: &str,
        ) -> Result<Option<String>, ServerCoreError> {
            Ok(self
                .rows
                .lock()
                .unwrap()
                .get(&(workspace_ark.to_string(), file_ark.to_string()))
                .map(|(object_key, _)| object_key.clone()))
        }
    }
    }

    #[tokio::test]
    async fn register_then_resolve() {
        let store = TestArkIndexStore::default();
        let service = ArkService::new(&store);

        service
            .register("dxbcdfgh6", "bcdfgr", "public/note.html", Some("public"))
            .await
            .unwrap();

        let entry = service.resolve("dxbcdfgh6", "bcdfgr").await.unwrap();
        assert_eq!(entry.object_key, "public/note.html");
        assert_eq!(entry.audience.as_deref(), Some("public"));
    }

    #[tokio::test]
    async fn resolve_missing_is_not_found() {
        let store = TestArkIndexStore::default();
        let service = ArkService::new(&store);
        let err = service.resolve("dxbcdfgh6", "bcdfgr").await.unwrap_err();
        assert!(matches!(err, ServerCoreError::NotFound(_)));
    }

    #[tokio::test]
    async fn register_is_idempotent_for_same_object() {
        let store = TestArkIndexStore::default();
        let service = ArkService::new(&store);

        service
            .register("dxbcdfgh6", "bcdfgr", "public/note.html", Some("public"))
            .await
            .unwrap();
        // Republishing the same file to the same key must succeed.
        service
            .register("dxbcdfgh6", "bcdfgr", "public/note.html", Some("public"))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn register_conflicts_on_different_object() {
        let store = TestArkIndexStore::default();
        let service = ArkService::new(&store);

        service
            .register("dxbcdfgh6", "bcdfgr", "public/note.html", Some("public"))
            .await
            .unwrap();

        // A second device minted the same provisional file blade for a
        // different file — must be rejected so the client remints.
        let err = service
            .register("dxbcdfgh6", "bcdfgr", "public/other.html", Some("public"))
            .await
            .unwrap_err();
        assert!(matches!(err, ServerCoreError::Conflict(_)));
    }
}
