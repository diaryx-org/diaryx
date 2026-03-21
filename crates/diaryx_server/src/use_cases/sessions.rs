use crate::domain::NamespaceSessionInfo;
use crate::ports::{NamespaceStore, ServerCoreError, SessionStore};

pub struct SessionService<'a> {
    namespace_store: &'a dyn NamespaceStore,
    session_store: &'a dyn SessionStore,
}

impl<'a> SessionService<'a> {
    pub fn new(
        namespace_store: &'a dyn NamespaceStore,
        session_store: &'a dyn SessionStore,
    ) -> Self {
        Self {
            namespace_store,
            session_store,
        }
    }

    pub async fn create(
        &self,
        namespace_id: &str,
        caller_user_id: &str,
        read_only: bool,
    ) -> Result<NamespaceSessionInfo, ServerCoreError> {
        let ns = self
            .namespace_store
            .get_namespace(namespace_id)
            .await?
            .ok_or_else(|| ServerCoreError::not_found("Namespace not found"))?;

        if ns.owner_user_id != caller_user_id {
            return Err(ServerCoreError::permission_denied(
                "You do not own this namespace",
            ));
        }

        let code = self
            .session_store
            .create_session(namespace_id, caller_user_id, read_only, None)
            .await?;

        Ok(NamespaceSessionInfo {
            code,
            namespace_id: namespace_id.to_string(),
            owner_user_id: caller_user_id.to_string(),
            read_only,
            created_at: 0, // actual timestamp set by store
            expires_at: None,
        })
    }

    pub async fn get(&self, code: &str) -> Result<NamespaceSessionInfo, ServerCoreError> {
        let code = code.to_uppercase();
        self.session_store
            .get_session(&code)
            .await?
            .ok_or_else(|| ServerCoreError::not_found("Session not found or expired"))
    }

    pub async fn update(
        &self,
        code: &str,
        read_only: bool,
        caller_user_id: &str,
    ) -> Result<(), ServerCoreError> {
        let code = code.to_uppercase();
        let session = self
            .session_store
            .get_session(&code)
            .await?
            .ok_or_else(|| ServerCoreError::not_found("Session not found or expired"))?;

        if session.owner_user_id != caller_user_id {
            return Err(ServerCoreError::permission_denied(
                "Only the session owner can update it",
            ));
        }

        let updated = self
            .session_store
            .update_session_read_only(&code, read_only)
            .await?;

        if !updated {
            return Err(ServerCoreError::not_found("Session not found"));
        }
        Ok(())
    }

    pub async fn delete(&self, code: &str, caller_user_id: &str) -> Result<(), ServerCoreError> {
        let code = code.to_uppercase();
        let session = self
            .session_store
            .get_session(&code)
            .await?
            .ok_or_else(|| ServerCoreError::not_found("Session not found or expired"))?;

        if session.owner_user_id != caller_user_id {
            return Err(ServerCoreError::permission_denied(
                "Only the session owner can end it",
            ));
        }

        let deleted = self.session_store.delete_session(&code).await?;
        if !deleted {
            return Err(ServerCoreError::not_found("Session not found"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::SessionService;
    use crate::domain::{AudienceInfo, CustomDomainInfo, NamespaceInfo, NamespaceSessionInfo};
    use crate::ports::{NamespaceStore, ServerCoreError, SessionStore};
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct TestNamespaceStore {
        namespaces: Mutex<HashMap<String, NamespaceInfo>>,
    }

    crate::cfg_async_trait! {
    impl NamespaceStore for TestNamespaceStore {
        async fn get_namespace(
            &self,
            namespace_id: &str,
        ) -> Result<Option<NamespaceInfo>, ServerCoreError> {
            Ok(self.namespaces.lock().unwrap().get(namespace_id).cloned())
        }
        async fn list_namespaces(
            &self,
            _: &str,
            _: u32,
            _: u32,
        ) -> Result<Vec<NamespaceInfo>, ServerCoreError> {
            Ok(vec![])
        }
        async fn create_namespace(&self, _: &str, _: &str, _: Option<&str>) -> Result<(), ServerCoreError> {
            Ok(())
        }
        async fn update_namespace_metadata(&self, _: &str, _: Option<&str>) -> Result<(), ServerCoreError> {
            Ok(())
        }
        async fn delete_namespace(&self, _: &str) -> Result<(), ServerCoreError> {
            Ok(())
        }
        async fn get_audience(
            &self,
            _: &str,
            _: &str,
        ) -> Result<Option<AudienceInfo>, ServerCoreError> {
            Ok(None)
        }
        async fn upsert_audience(&self, _: &str, _: &str, _: &str) -> Result<(), ServerCoreError> {
            Ok(())
        }
        async fn list_audiences(&self, _: &str) -> Result<Vec<AudienceInfo>, ServerCoreError> {
            Ok(vec![])
        }
        async fn delete_audience(&self, _: &str, _: &str) -> Result<(), ServerCoreError> {
            Ok(())
        }
        async fn clear_objects_audience(&self, _: &str, _: &str) -> Result<(), ServerCoreError> {
            Ok(())
        }
        async fn get_custom_domain(
            &self,
            _: &str,
        ) -> Result<Option<CustomDomainInfo>, ServerCoreError> {
            Ok(None)
        }
        async fn list_custom_domains(
            &self,
            _: &str,
        ) -> Result<Vec<CustomDomainInfo>, ServerCoreError> {
            Ok(vec![])
        }
        async fn upsert_custom_domain(
            &self,
            _: &str,
            _: &str,
            _: &str,
        ) -> Result<(), ServerCoreError> {
            Ok(())
        }
        async fn delete_custom_domain(&self, _: &str) -> Result<bool, ServerCoreError> {
            Ok(false)
        }
    }
    }

    #[derive(Default)]
    struct TestSessionStore {
        sessions: Mutex<HashMap<String, NamespaceSessionInfo>>,
        next_code: Mutex<u32>,
    }

    crate::cfg_async_trait! {
    impl SessionStore for TestSessionStore {
        async fn create_session(
            &self,
            namespace_id: &str,
            owner_user_id: &str,
            read_only: bool,
            expires_at: Option<i64>,
        ) -> Result<String, ServerCoreError> {
            let mut counter = self.next_code.lock().unwrap();
            *counter += 1;
            let code = format!("CODE-{:04}", counter);
            self.sessions.lock().unwrap().insert(
                code.clone(),
                NamespaceSessionInfo {
                    code: code.clone(),
                    namespace_id: namespace_id.to_string(),
                    owner_user_id: owner_user_id.to_string(),
                    read_only,
                    created_at: 1,
                    expires_at,
                },
            );
            Ok(code)
        }

        async fn get_session(
            &self,
            code: &str,
        ) -> Result<Option<NamespaceSessionInfo>, ServerCoreError> {
            Ok(self.sessions.lock().unwrap().get(code).cloned())
        }

        async fn update_session_read_only(
            &self,
            code: &str,
            read_only: bool,
        ) -> Result<bool, ServerCoreError> {
            if let Some(session) = self.sessions.lock().unwrap().get_mut(code) {
                session.read_only = read_only;
                Ok(true)
            } else {
                Ok(false)
            }
        }

        async fn delete_session(&self, code: &str) -> Result<bool, ServerCoreError> {
            Ok(self.sessions.lock().unwrap().remove(code).is_some())
        }
    }
    }

    fn make_stores(owner: &str, ns_id: &str) -> (TestNamespaceStore, TestSessionStore) {
        let ns_store = TestNamespaceStore::default();
        ns_store.namespaces.lock().unwrap().insert(
            ns_id.to_string(),
            NamespaceInfo {
                id: ns_id.to_string(),
                owner_user_id: owner.to_string(),
                created_at: 1,
                metadata: None,
            },
        );
        (ns_store, TestSessionStore::default())
    }

    #[tokio::test]
    async fn create_session_checks_ownership() {
        let (ns_store, session_store) = make_stores("user1", "ns1");
        let service = SessionService::new(&ns_store, &session_store);

        let err = service.create("ns1", "user2", false).await.unwrap_err();
        assert!(matches!(err, ServerCoreError::PermissionDenied(_)));
    }

    #[tokio::test]
    async fn create_and_get_session() {
        let (ns_store, session_store) = make_stores("user1", "ns1");
        let service = SessionService::new(&ns_store, &session_store);

        let session = service.create("ns1", "user1", false).await.unwrap();
        assert_eq!(session.namespace_id, "ns1");
        assert!(!session.read_only);

        let fetched = service.get(&session.code).await.unwrap();
        assert_eq!(fetched.namespace_id, "ns1");
    }

    #[tokio::test]
    async fn update_rejects_non_owner() {
        let (ns_store, session_store) = make_stores("user1", "ns1");
        let service = SessionService::new(&ns_store, &session_store);

        let session = service.create("ns1", "user1", false).await.unwrap();

        let err = service
            .update(&session.code, true, "user2")
            .await
            .unwrap_err();
        assert!(matches!(err, ServerCoreError::PermissionDenied(_)));
    }

    #[tokio::test]
    async fn delete_removes_session() {
        let (ns_store, session_store) = make_stores("user1", "ns1");
        let service = SessionService::new(&ns_store, &session_store);

        let session = service.create("ns1", "user1", false).await.unwrap();
        service.delete(&session.code, "user1").await.unwrap();

        let err = service.get(&session.code).await.unwrap_err();
        assert!(matches!(err, ServerCoreError::NotFound(_)));
    }

    #[tokio::test]
    async fn get_normalizes_code_to_uppercase() {
        let (ns_store, session_store) = make_stores("user1", "ns1");
        let service = SessionService::new(&ns_store, &session_store);

        let session = service.create("ns1", "user1", false).await.unwrap();
        let lower = session.code.to_lowercase();
        // The test store stores uppercase codes, so lowercase won't match.
        // This verifies the service normalizes input.
        let result = service.get(&lower).await;
        // The code was stored as uppercase CODE-0001, so lowercased code-0001
        // gets uppercased back to CODE-0001 and should be found.
        assert!(result.is_ok());
    }
}
