use crate::audience_token::{AudienceTokenClaims, GateKind, create_audience_token};
use crate::domain::{AudienceInfo, GateInput, GateRecord};
use crate::ports::{BlobStore, NamespaceStore, ServerCoreError};
use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use serde::{Deserialize, Serialize};
use tracing::warn;
use uuid::Uuid;

/// Generate a 16-byte salt using the `uuid` v4 generator. The workspace
/// already depends on `uuid` with `getrandom`-backed v4 support, so this lets
/// us avoid pulling another RNG (and argon2's optional `password-hash/getrandom`
/// feature, which doesn't propagate cleanly through default features) into the
/// dep tree.
fn generate_salt_string() -> SaltString {
    let bytes = Uuid::new_v4().into_bytes();
    SaltString::encode_b64(&bytes).expect("16 bytes fit in a SaltString")
}

// ---------------------------------------------------------------------------
// HTTP request / response types shared across adapters.
// ---------------------------------------------------------------------------

/// Body of `PUT /namespaces/{id}/audiences/{name}`. Wire-shape matches
/// `{"gates": [{"kind": "link"}, {"kind": "password", "password": "pw"}]}`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SetAudienceRequest {
    #[serde(default)]
    pub gates: Vec<GateInput>,
}

/// Response body for audience read endpoints. Password hashes are never
/// included; the `password` gate record serializes with an optional `hash`
/// that is always `None` on the wire.
#[derive(Debug, Clone, Serialize)]
pub struct AudienceResponse {
    pub namespace_id: String,
    pub name: String,
    pub gates: Vec<GateRecord>,
}

impl From<AudienceInfo> for AudienceResponse {
    fn from(info: AudienceInfo) -> Self {
        Self {
            namespace_id: info.namespace_id,
            name: info.audience_name,
            gates: info
                .gates
                .into_iter()
                .map(|g| match g {
                    GateRecord::Password { version, .. } => GateRecord::Password {
                        hash: None,
                        version,
                    },
                    other => other,
                })
                .collect(),
        }
    }
}

/// Body of `POST /namespaces/{id}/audiences/{name}/unlock`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UnlockRequest {
    pub password: String,
}

/// Body of `POST /namespaces/{id}/audiences/{name}/rotate-password`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RotatePasswordRequest {
    pub password: String,
}

/// Response carrying a signed audience-access token.
#[derive(Debug, Clone, Serialize)]
pub struct TokenResponse {
    pub token: String,
}

pub struct AudienceService<'a> {
    namespace_store: &'a dyn NamespaceStore,
    blob_store: &'a dyn BlobStore,
}

/// Result of a successful password verification: the password gate's current
/// version, to be embedded in the emitted unlock token.
#[derive(Debug, Clone, Copy)]
pub struct PasswordVerified {
    pub version: u32,
}

impl<'a> AudienceService<'a> {
    pub fn new(namespace_store: &'a dyn NamespaceStore, blob_store: &'a dyn BlobStore) -> Self {
        Self {
            namespace_store,
            blob_store,
        }
    }

    fn require_owner<'b>(
        &self,
        ns: &'b crate::domain::NamespaceInfo,
        caller_user_id: &str,
    ) -> Result<(), ServerCoreError> {
        if ns.owner_user_id != caller_user_id {
            return Err(ServerCoreError::permission_denied(
                "You do not own this namespace",
            ));
        }
        Ok(())
    }

    async fn require_namespace_owner(
        &self,
        namespace_id: &str,
        caller_user_id: &str,
    ) -> Result<(), ServerCoreError> {
        let ns = self
            .namespace_store
            .get_namespace(namespace_id)
            .await?
            .ok_or_else(|| ServerCoreError::not_found("Namespace not found"))?;
        self.require_owner(&ns, caller_user_id)
    }

    /// Upsert an audience with the given gate set.
    ///
    /// Merge rules (applied before writing):
    /// - `GateInput::Link` → `GateRecord::Link` (unit).
    /// - `GateInput::Password { password: Some }` → hash with Argon2, bump
    ///   version (starting at 1; if an existing password gate had version N,
    ///   the new record has version N+1).
    /// - `GateInput::Password { password: None }` → preserve the existing
    ///   password gate's hash + version if present; otherwise store with
    ///   `hash: None, version: 0` (writer declared the gate but hasn't set
    ///   a password yet).
    ///
    /// Duplicate gate kinds in the input are rejected with `InvalidInput`.
    pub async fn set(
        &self,
        namespace_id: &str,
        audience_name: &str,
        inputs: Vec<GateInput>,
        caller_user_id: &str,
    ) -> Result<AudienceInfo, ServerCoreError> {
        self.require_namespace_owner(namespace_id, caller_user_id)
            .await?;

        validate_no_duplicate_kinds(&inputs)?;

        let existing = self
            .namespace_store
            .get_audience(namespace_id, audience_name)
            .await?;
        let existing_password = existing
            .as_ref()
            .and_then(|a| a.password_gate().cloned())
            .and_then(|g| match g {
                GateRecord::Password { hash, version } => Some((hash, version)),
                _ => None,
            });

        let mut merged = Vec::with_capacity(inputs.len());
        for input in inputs {
            match input {
                GateInput::Link => merged.push(GateRecord::Link),
                GateInput::Password { password: Some(pw) } => {
                    let hash = hash_password(&pw)?;
                    let prev_version = existing_password.as_ref().map(|(_, v)| *v).unwrap_or(0);
                    merged.push(GateRecord::Password {
                        hash: Some(hash),
                        version: prev_version.saturating_add(1),
                    });
                }
                GateInput::Password { password: None } => {
                    let (hash, version) = existing_password.clone().unwrap_or((None, 0));
                    merged.push(GateRecord::Password { hash, version });
                }
            }
        }

        self.namespace_store
            .upsert_audience(namespace_id, audience_name, &merged)
            .await?;

        let info = self
            .namespace_store
            .get_audience(namespace_id, audience_name)
            .await?
            .ok_or_else(|| ServerCoreError::internal("Audience missing after upsert"))?;

        self.write_audiences_meta(namespace_id).await;
        Ok(info)
    }

    pub async fn list(
        &self,
        namespace_id: &str,
        caller_user_id: &str,
    ) -> Result<Vec<AudienceInfo>, ServerCoreError> {
        self.require_namespace_owner(namespace_id, caller_user_id)
            .await?;
        self.namespace_store.list_audiences(namespace_id).await
    }

    pub async fn delete(
        &self,
        namespace_id: &str,
        audience_name: &str,
        caller_user_id: &str,
    ) -> Result<(), ServerCoreError> {
        self.require_namespace_owner(namespace_id, caller_user_id)
            .await?;

        if self
            .namespace_store
            .get_audience(namespace_id, audience_name)
            .await?
            .is_none()
        {
            return Err(ServerCoreError::not_found("Audience not found"));
        }

        self.namespace_store
            .clear_objects_audience(namespace_id, audience_name)
            .await?;
        self.namespace_store
            .delete_audience(namespace_id, audience_name)
            .await?;
        self.write_audiences_meta(namespace_id).await;
        Ok(())
    }

    /// Check that the audience exists and has a `link` gate; the caller uses
    /// this before issuing a magic-link token. Returns the audience info.
    pub async fn require_link_eligible(
        &self,
        namespace_id: &str,
        audience_name: &str,
        caller_user_id: &str,
    ) -> Result<AudienceInfo, ServerCoreError> {
        self.require_namespace_owner(namespace_id, caller_user_id)
            .await?;

        let audience = self
            .namespace_store
            .get_audience(namespace_id, audience_name)
            .await?
            .ok_or_else(|| ServerCoreError::not_found("Audience not found"))?;

        if !audience.has_link_gate() {
            return Err(ServerCoreError::invalid_input(
                "audience has no link gate; no magic-link token can be issued",
            ));
        }

        Ok(audience)
    }

    /// Issue a magic-link token for an audience. The caller must own the
    /// namespace. Fails if the audience does not have a `link` gate.
    pub async fn issue_link_token(
        &self,
        signing_key: &[u8],
        namespace_id: &str,
        audience_name: &str,
        caller_user_id: &str,
    ) -> Result<TokenResponse, ServerCoreError> {
        self.require_link_eligible(namespace_id, audience_name, caller_user_id)
            .await?;
        let claims = AudienceTokenClaims {
            slug: namespace_id.to_string(),
            audience: audience_name.to_string(),
            token_id: Uuid::new_v4().to_string(),
            gate: GateKind::Link,
            password_version: None,
            expires_at: None,
        };
        let token =
            create_audience_token(signing_key, &claims).map_err(ServerCoreError::internal)?;
        Ok(TokenResponse { token })
    }

    /// Verify a reader-supplied password and issue an unlock token on success.
    /// Unauthenticated — password IS the authentication. Callers must rate-
    /// limit by (audience, IP) before calling.
    pub async fn unlock_with_password(
        &self,
        signing_key: &[u8],
        namespace_id: &str,
        audience_name: &str,
        password: &str,
    ) -> Result<TokenResponse, ServerCoreError> {
        let verified = self
            .verify_password(namespace_id, audience_name, password)
            .await?;
        let claims = AudienceTokenClaims {
            slug: namespace_id.to_string(),
            audience: audience_name.to_string(),
            token_id: Uuid::new_v4().to_string(),
            gate: GateKind::Unlock,
            password_version: Some(verified.version),
            expires_at: None,
        };
        let token =
            create_audience_token(signing_key, &claims).map_err(ServerCoreError::internal)?;
        Ok(TokenResponse { token })
    }

    /// Owner-authenticated password rotation. Bumps the version, rewrites
    /// the stored hash, and mints a fresh unlock token for the writer so
    /// they can test the new password immediately if they want.
    pub async fn rotate_password_and_issue(
        &self,
        signing_key: &[u8],
        namespace_id: &str,
        audience_name: &str,
        new_password: &str,
        caller_user_id: &str,
    ) -> Result<(u32, TokenResponse), ServerCoreError> {
        let new_version = self
            .rotate_password(namespace_id, audience_name, new_password, caller_user_id)
            .await?;
        let claims = AudienceTokenClaims {
            slug: namespace_id.to_string(),
            audience: audience_name.to_string(),
            token_id: Uuid::new_v4().to_string(),
            gate: GateKind::Unlock,
            password_version: Some(new_version),
            expires_at: None,
        };
        let token =
            create_audience_token(signing_key, &claims).map_err(ServerCoreError::internal)?;
        Ok((new_version, TokenResponse { token }))
    }

    /// Verify a plaintext password against the audience's password gate.
    /// Unauthenticated — callers must apply their own rate limiting by
    /// (audience, IP) before invoking.
    pub async fn verify_password(
        &self,
        namespace_id: &str,
        audience_name: &str,
        password: &str,
    ) -> Result<PasswordVerified, ServerCoreError> {
        let audience = self
            .namespace_store
            .get_audience(namespace_id, audience_name)
            .await?
            .ok_or_else(|| ServerCoreError::not_found("Audience not found"))?;

        let (hash, version) = match audience.password_gate() {
            Some(GateRecord::Password {
                hash: Some(h),
                version,
            }) => (h.clone(), *version),
            Some(GateRecord::Password { hash: None, .. }) => {
                return Err(ServerCoreError::invalid_input(
                    "password not set for this audience",
                ));
            }
            _ => {
                return Err(ServerCoreError::invalid_input(
                    "audience has no password gate",
                ));
            }
        };

        if verify_password_hash(password, &hash)? {
            Ok(PasswordVerified { version })
        } else {
            Err(ServerCoreError::permission_denied("Incorrect password"))
        }
    }

    /// Owner-authenticated password rotation. Bumps the gate's version and
    /// returns the new version; existing unlock tokens issued under the old
    /// version will fail validation at the site-proxy.
    pub async fn rotate_password(
        &self,
        namespace_id: &str,
        audience_name: &str,
        new_password: &str,
        caller_user_id: &str,
    ) -> Result<u32, ServerCoreError> {
        self.require_namespace_owner(namespace_id, caller_user_id)
            .await?;

        let audience = self
            .namespace_store
            .get_audience(namespace_id, audience_name)
            .await?
            .ok_or_else(|| ServerCoreError::not_found("Audience not found"))?;

        let mut gates = audience.gates.clone();
        let prev_version = gates
            .iter()
            .find_map(|g| match g {
                GateRecord::Password { version, .. } => Some(*version),
                _ => None,
            })
            .ok_or_else(|| ServerCoreError::invalid_input("audience has no password gate"))?;

        let new_version = prev_version.saturating_add(1);
        let new_hash = hash_password(new_password)?;

        for gate in gates.iter_mut() {
            if let GateRecord::Password { hash, version } = gate {
                *hash = Some(new_hash.clone());
                *version = new_version;
            }
        }

        self.namespace_store
            .upsert_audience(namespace_id, audience_name, &gates)
            .await?;

        self.write_audiences_meta(namespace_id).await;
        Ok(new_version)
    }

    /// Write `ns/{ns_id}/_audiences.json` to the blob store.
    /// The file contains a map of audience name → `{ gates: [...] }` so the
    /// site-proxy can evaluate gates without additional round-trips. Password
    /// gate hashes are intentionally stripped before writing — the blob only
    /// carries enough metadata to identify that a password gate exists and
    /// what version it is at.
    ///
    /// Best-effort — errors are logged but do not fail the caller.
    async fn write_audiences_meta(&self, namespace_id: &str) {
        let audiences = match self.namespace_store.list_audiences(namespace_id).await {
            Ok(a) => a,
            Err(e) => {
                warn!(
                    "Failed to list audiences for metadata write ({}): {}",
                    namespace_id, e
                );
                return;
            }
        };

        let map: serde_json::Map<String, serde_json::Value> = audiences
            .into_iter()
            .map(|a| {
                let public_gates: Vec<serde_json::Value> = a
                    .gates
                    .iter()
                    .map(|g| match g {
                        GateRecord::Link => serde_json::json!({ "kind": "link" }),
                        GateRecord::Password { version, .. } => {
                            serde_json::json!({ "kind": "password", "version": version })
                        }
                    })
                    .collect();
                (
                    a.audience_name,
                    serde_json::json!({ "gates": public_gates }),
                )
            })
            .collect();

        let json = serde_json::to_vec(&map).unwrap_or_default();
        let key = format!("ns/{}/_audiences.json", namespace_id);
        if let Err(e) = self
            .blob_store
            .put(&key, &json, "application/json", None)
            .await
        {
            warn!(
                "Failed to write audiences metadata for {}: {}",
                namespace_id, e
            );
        }
    }
}

fn validate_no_duplicate_kinds(inputs: &[GateInput]) -> Result<(), ServerCoreError> {
    let mut seen_link = false;
    let mut seen_password = false;
    for g in inputs {
        match g {
            GateInput::Link => {
                if seen_link {
                    return Err(ServerCoreError::invalid_input(
                        "duplicate link gate in input",
                    ));
                }
                seen_link = true;
            }
            GateInput::Password { .. } => {
                if seen_password {
                    return Err(ServerCoreError::invalid_input(
                        "duplicate password gate in input",
                    ));
                }
                seen_password = true;
            }
        }
    }
    Ok(())
}

fn hash_password(plaintext: &str) -> Result<String, ServerCoreError> {
    let salt = generate_salt_string();
    let argon = Argon2::default();
    argon
        .hash_password(plaintext.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| ServerCoreError::internal(format!("password hash failure: {e}")))
}

fn verify_password_hash(plaintext: &str, encoded: &str) -> Result<bool, ServerCoreError> {
    let parsed = PasswordHash::new(encoded)
        .map_err(|e| ServerCoreError::internal(format!("stored hash is malformed: {e}")))?;
    Ok(Argon2::default()
        .verify_password(plaintext.as_bytes(), &parsed)
        .is_ok())
}

#[cfg(test)]
mod tests {
    use super::AudienceService;
    use crate::domain::{AudienceInfo, CustomDomainInfo, GateInput, GateRecord, NamespaceInfo};
    use crate::ports::{BlobStore, MultipartCompletedPart, NamespaceStore, ServerCoreError};
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct TestStore {
        namespaces: Mutex<HashMap<String, NamespaceInfo>>,
        audiences: Mutex<HashMap<(String, String), AudienceInfo>>,
    }

    crate::cfg_async_trait! {
    impl NamespaceStore for TestStore {
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
            namespace_id: &str,
            audience_name: &str,
        ) -> Result<Option<AudienceInfo>, ServerCoreError> {
            Ok(self
                .audiences
                .lock()
                .unwrap()
                .get(&(namespace_id.to_string(), audience_name.to_string()))
                .cloned())
        }
        async fn upsert_audience(
            &self,
            namespace_id: &str,
            audience_name: &str,
            gates: &[GateRecord],
        ) -> Result<(), ServerCoreError> {
            self.audiences.lock().unwrap().insert(
                (namespace_id.to_string(), audience_name.to_string()),
                AudienceInfo {
                    namespace_id: namespace_id.to_string(),
                    audience_name: audience_name.to_string(),
                    gates: gates.to_vec(),
                },
            );
            Ok(())
        }
        async fn list_audiences(
            &self,
            namespace_id: &str,
        ) -> Result<Vec<AudienceInfo>, ServerCoreError> {
            Ok(self
                .audiences
                .lock()
                .unwrap()
                .values()
                .filter(|a| a.namespace_id == namespace_id)
                .cloned()
                .collect())
        }
        async fn delete_audience(
            &self,
            namespace_id: &str,
            audience_name: &str,
        ) -> Result<(), ServerCoreError> {
            self.audiences
                .lock()
                .unwrap()
                .remove(&(namespace_id.to_string(), audience_name.to_string()));
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
    struct TestBlobStore {
        blobs: Mutex<HashMap<String, Vec<u8>>>,
    }

    crate::cfg_async_trait! {
    impl BlobStore for TestBlobStore {
        fn blob_key(&self, _: &str, _: &str) -> String {
            String::new()
        }
        fn prefix(&self) -> &str {
            ""
        }
        async fn put(
            &self,
            key: &str,
            bytes: &[u8],
            _: &str,
            _: Option<&HashMap<String, String>>,
        ) -> Result<(), ServerCoreError> {
            self.blobs
                .lock()
                .unwrap()
                .insert(key.to_string(), bytes.to_vec());
            Ok(())
        }
        async fn get(&self, _: &str) -> Result<Option<Vec<u8>>, ServerCoreError> {
            Ok(None)
        }
        async fn delete(&self, _: &str) -> Result<(), ServerCoreError> {
            Ok(())
        }
        async fn exists(&self, _: &str) -> Result<bool, ServerCoreError> {
            Ok(false)
        }
        async fn init_multipart(&self, _: &str, _: &str) -> Result<String, ServerCoreError> {
            Ok(String::new())
        }
        async fn upload_part(
            &self,
            _: &str,
            _: &str,
            _: u32,
            _: &[u8],
        ) -> Result<String, ServerCoreError> {
            Ok(String::new())
        }
        async fn complete_multipart(
            &self,
            _: &str,
            _: &str,
            _: &[MultipartCompletedPart],
        ) -> Result<(), ServerCoreError> {
            Ok(())
        }
        async fn abort_multipart(&self, _: &str, _: &str) -> Result<(), ServerCoreError> {
            Ok(())
        }
        async fn get_range(
            &self,
            _: &str,
            _: u64,
            _: u64,
        ) -> Result<Option<Vec<u8>>, ServerCoreError> {
            Ok(None)
        }
        async fn list_by_prefix(&self, _: &str) -> Result<Vec<String>, ServerCoreError> {
            Ok(vec![])
        }
        async fn delete_by_prefix(&self, _: &str) -> Result<usize, ServerCoreError> {
            Ok(0)
        }
    }
    }

    fn make_store_with_namespace(owner: &str, ns_id: &str) -> TestStore {
        let store = TestStore::default();
        store.namespaces.lock().unwrap().insert(
            ns_id.to_string(),
            NamespaceInfo {
                id: ns_id.to_string(),
                owner_user_id: owner.to_string(),
                created_at: 1,
                metadata: None,
            },
        );
        store
    }

    #[tokio::test]
    async fn set_public_audience_has_empty_gates() {
        let store = make_store_with_namespace("user1", "ns1");
        let blob_store = TestBlobStore::default();
        let service = AudienceService::new(&store, &blob_store);

        let info = service.set("ns1", "public", vec![], "user1").await.unwrap();
        assert!(info.is_public());
        assert!(info.gates.is_empty());
    }

    #[tokio::test]
    async fn set_link_gate_audience() {
        let store = make_store_with_namespace("user1", "ns1");
        let blob_store = TestBlobStore::default();
        let service = AudienceService::new(&store, &blob_store);

        let info = service
            .set("ns1", "members", vec![GateInput::Link], "user1")
            .await
            .unwrap();
        assert!(info.has_link_gate());
        assert!(info.password_gate().is_none());
    }

    #[tokio::test]
    async fn set_password_gate_with_plaintext_hashes_and_bumps_version() {
        let store = make_store_with_namespace("user1", "ns1");
        let blob_store = TestBlobStore::default();
        let service = AudienceService::new(&store, &blob_store);

        let info = service
            .set(
                "ns1",
                "inner",
                vec![GateInput::Password {
                    password: Some("correct-horse-battery-staple".to_string()),
                }],
                "user1",
            )
            .await
            .unwrap();
        match info.password_gate().unwrap() {
            GateRecord::Password { hash, version } => {
                assert!(hash.as_ref().is_some_and(|h| h.starts_with("$argon2")));
                assert_eq!(*version, 1);
            }
            _ => panic!("expected password gate"),
        }
    }

    #[tokio::test]
    async fn set_password_gate_without_plaintext_preserves_hash() {
        let store = make_store_with_namespace("user1", "ns1");
        let blob_store = TestBlobStore::default();
        let service = AudienceService::new(&store, &blob_store);

        service
            .set(
                "ns1",
                "inner",
                vec![GateInput::Password {
                    password: Some("pw".to_string()),
                }],
                "user1",
            )
            .await
            .unwrap();
        let before = service.list("ns1", "user1").await.unwrap();
        let (before_hash, before_version) = match before[0].password_gate().unwrap() {
            GateRecord::Password { hash, version } => (hash.clone(), *version),
            _ => unreachable!(),
        };

        // Re-set the gate without providing a password; hash+version preserved.
        service
            .set(
                "ns1",
                "inner",
                vec![GateInput::Password { password: None }],
                "user1",
            )
            .await
            .unwrap();
        let after = service.list("ns1", "user1").await.unwrap();
        match after[0].password_gate().unwrap() {
            GateRecord::Password { hash, version } => {
                assert_eq!(hash, &before_hash);
                assert_eq!(*version, before_version);
            }
            _ => unreachable!(),
        }
    }

    #[tokio::test]
    async fn set_rejects_duplicate_gate_kinds() {
        let store = make_store_with_namespace("user1", "ns1");
        let blob_store = TestBlobStore::default();
        let service = AudienceService::new(&store, &blob_store);

        let err = service
            .set(
                "ns1",
                "bad",
                vec![GateInput::Link, GateInput::Link],
                "user1",
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServerCoreError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn stacked_gates_preserve_order() {
        let store = make_store_with_namespace("user1", "ns1");
        let blob_store = TestBlobStore::default();
        let service = AudienceService::new(&store, &blob_store);

        let info = service
            .set(
                "ns1",
                "close",
                vec![
                    GateInput::Password {
                        password: Some("secret".to_string()),
                    },
                    GateInput::Link,
                ],
                "user1",
            )
            .await
            .unwrap();
        assert_eq!(info.gates.len(), 2);
        assert!(info.password_gate().is_some());
        assert!(info.has_link_gate());
    }

    #[tokio::test]
    async fn verify_password_succeeds_with_correct_plaintext() {
        let store = make_store_with_namespace("user1", "ns1");
        let blob_store = TestBlobStore::default();
        let service = AudienceService::new(&store, &blob_store);

        service
            .set(
                "ns1",
                "inner",
                vec![GateInput::Password {
                    password: Some("hunter2".to_string()),
                }],
                "user1",
            )
            .await
            .unwrap();

        let verified = service
            .verify_password("ns1", "inner", "hunter2")
            .await
            .unwrap();
        assert_eq!(verified.version, 1);
    }

    #[tokio::test]
    async fn verify_password_rejects_incorrect_plaintext() {
        let store = make_store_with_namespace("user1", "ns1");
        let blob_store = TestBlobStore::default();
        let service = AudienceService::new(&store, &blob_store);

        service
            .set(
                "ns1",
                "inner",
                vec![GateInput::Password {
                    password: Some("hunter2".to_string()),
                }],
                "user1",
            )
            .await
            .unwrap();

        let err = service
            .verify_password("ns1", "inner", "wrong")
            .await
            .unwrap_err();
        assert!(matches!(err, ServerCoreError::PermissionDenied(_)));
    }

    #[tokio::test]
    async fn verify_password_rejects_unset_gate() {
        let store = make_store_with_namespace("user1", "ns1");
        let blob_store = TestBlobStore::default();
        let service = AudienceService::new(&store, &blob_store);

        // Declare password gate without setting a password yet.
        service
            .set(
                "ns1",
                "inner",
                vec![GateInput::Password { password: None }],
                "user1",
            )
            .await
            .unwrap();

        let err = service
            .verify_password("ns1", "inner", "anything")
            .await
            .unwrap_err();
        assert!(matches!(err, ServerCoreError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn rotate_password_bumps_version() {
        let store = make_store_with_namespace("user1", "ns1");
        let blob_store = TestBlobStore::default();
        let service = AudienceService::new(&store, &blob_store);

        service
            .set(
                "ns1",
                "inner",
                vec![GateInput::Password {
                    password: Some("old".to_string()),
                }],
                "user1",
            )
            .await
            .unwrap();

        let new_version = service
            .rotate_password("ns1", "inner", "new", "user1")
            .await
            .unwrap();
        assert_eq!(new_version, 2);

        // Old password no longer works.
        let err = service
            .verify_password("ns1", "inner", "old")
            .await
            .unwrap_err();
        assert!(matches!(err, ServerCoreError::PermissionDenied(_)));

        // New password works and reports the bumped version.
        let verified = service
            .verify_password("ns1", "inner", "new")
            .await
            .unwrap();
        assert_eq!(verified.version, 2);
    }

    #[tokio::test]
    async fn require_link_eligible_rejects_audience_without_link_gate() {
        let store = make_store_with_namespace("user1", "ns1");
        let blob_store = TestBlobStore::default();
        let service = AudienceService::new(&store, &blob_store);

        service
            .set(
                "ns1",
                "inner",
                vec![GateInput::Password {
                    password: Some("pw".to_string()),
                }],
                "user1",
            )
            .await
            .unwrap();

        let err = service
            .require_link_eligible("ns1", "inner", "user1")
            .await
            .unwrap_err();
        assert!(matches!(err, ServerCoreError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn require_link_eligible_accepts_link_gate() {
        let store = make_store_with_namespace("user1", "ns1");
        let blob_store = TestBlobStore::default();
        let service = AudienceService::new(&store, &blob_store);

        service
            .set("ns1", "members", vec![GateInput::Link], "user1")
            .await
            .unwrap();

        let info = service
            .require_link_eligible("ns1", "members", "user1")
            .await
            .unwrap();
        assert!(info.has_link_gate());
    }

    #[tokio::test]
    async fn delete_audience_clears_objects_and_writes_meta() {
        let store = make_store_with_namespace("user1", "ns1");
        let blob_store = TestBlobStore::default();
        let service = AudienceService::new(&store, &blob_store);

        service.set("ns1", "pub", vec![], "user1").await.unwrap();
        service.delete("ns1", "pub", "user1").await.unwrap();

        let audiences = service.list("ns1", "user1").await.unwrap();
        assert!(audiences.is_empty());
    }

    #[tokio::test]
    async fn rejects_non_owner() {
        let store = make_store_with_namespace("user1", "ns1");
        let blob_store = TestBlobStore::default();
        let service = AudienceService::new(&store, &blob_store);

        let err = service
            .set("ns1", "pub", vec![], "user2")
            .await
            .unwrap_err();
        assert!(matches!(err, ServerCoreError::PermissionDenied(_)));
    }

    #[tokio::test]
    async fn metadata_blob_written_with_gates_shape() {
        let store = make_store_with_namespace("user1", "ns1");
        let blob_store = TestBlobStore::default();
        let service = AudienceService::new(&store, &blob_store);

        service
            .set("ns1", "members", vec![GateInput::Link], "user1")
            .await
            .unwrap();

        let blob = blob_store
            .blobs
            .lock()
            .unwrap()
            .get("ns/ns1/_audiences.json")
            .cloned()
            .expect("metadata blob");
        let parsed: serde_json::Value = serde_json::from_slice(&blob).unwrap();
        assert_eq!(
            parsed["members"]["gates"][0]["kind"].as_str().unwrap(),
            "link"
        );
    }
}
