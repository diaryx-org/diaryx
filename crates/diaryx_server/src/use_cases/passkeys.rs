//! Portable passkey (WebAuthn) registration and authentication service.
//!
//! Uses `webauthn_rp` for server-side ceremony verification. Both native
//! (Axum/SQLite) and Cloudflare Workers adapters share this implementation.

use crate::domain::PasskeyInfo;
use crate::ports::{PasskeyStore, ServerCoreError};
use base64::Engine;
use tracing::info;
use webauthn_rp::bin::{Decode, Encode};
use webauthn_rp::request::register::{USER_HANDLE_MAX_LEN, UserHandle64};
use webauthn_rp::request::{AsciiDomain, RpId};
use webauthn_rp::response::register::{CompressedPubKey, DynamicState, StaticState};
use webauthn_rp::response::{AuthTransports, CredentialId};
use webauthn_rp::{
    AuthenticatedCredential, DiscoverableAuthentication64, DiscoverableAuthenticationServerState,
    DiscoverableCredentialRequestOptions, PublicKeyCredentialCreationOptions, Registration,
    RegistrationServerState,
};

const B64: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::STANDARD;

type PubKey = CompressedPubKey<[u8; 32], [u8; 32], [u8; 48], Vec<u8>>;

fn enc_err(e: impl std::fmt::Display) -> ServerCoreError {
    ServerCoreError::internal(format!("Encode error: {e}"))
}

fn dec_err(e: impl std::fmt::Display) -> ServerCoreError {
    ServerCoreError::internal(format!("Decode error: {e}"))
}

/// Portable passkey service backed by the `PasskeyStore` trait.
pub struct PasskeyService<'a> {
    store: &'a dyn PasskeyStore,
    rp_id: String,
}

/// Stored credential data (serialized via webauthn_rp's binary encoding).
#[derive(serde::Serialize, serde::Deserialize)]
struct StoredCredential {
    /// Base64-encoded CredentialId.
    cred_id: String,
    /// Base64-encoded UserHandle64 (64 bytes).
    user_handle: String,
    /// Base64-encoded StaticState (public key + algorithm).
    static_state: String,
    /// Base64-encoded DynamicState (counter, backup flags).
    dynamic_state: String,
}

/// Stored server-side challenge state.
#[derive(serde::Serialize, serde::Deserialize)]
struct StoredChallengeState {
    /// Base64-encoded server state binary.
    state: String,
}

/// Result of a successful passkey authentication.
pub struct PasskeyAuthResult {
    pub user_id: String,
    pub email: String,
}

impl<'a> PasskeyService<'a> {
    pub fn new(store: &'a dyn PasskeyStore, rp_id: &str) -> Self {
        Self {
            store,
            rp_id: rp_id.to_string(),
        }
    }

    fn rp_id(&self) -> Result<RpId, ServerCoreError> {
        let domain = AsciiDomain::try_from(self.rp_id.clone())
            .map_err(|_| ServerCoreError::internal(format!("Invalid RP ID: {}", self.rp_id)))?;
        Ok(RpId::Domain(domain))
    }

    // ========================================================================
    // Registration
    // ========================================================================

    /// Start passkey registration for an authenticated user.
    /// Returns (challenge_id, options_json) to send to the client.
    pub async fn start_registration(
        &self,
        user_id: &str,
        email: &str,
    ) -> Result<(String, serde_json::Value), ServerCoreError> {
        let rp_id = self.rp_id()?;

        // Generate a random user handle (64 bytes as recommended by spec).
        let user_handle = UserHandle64::new();

        // Get existing credential IDs to exclude
        let existing = self.store.get_credentials(user_id).await?;
        let exclude_creds: Vec<webauthn_rp::request::PublicKeyCredentialDescriptor<Vec<u8>>> =
            existing
                .iter()
                .filter_map(|c| {
                    let stored: StoredCredential = serde_json::from_str(&c.credential_json).ok()?;
                    let cred_bytes = B64.decode(&stored.cred_id).ok()?;
                    let cred_id = CredentialId::<Vec<u8>>::decode(cred_bytes).ok()?;
                    Some(webauthn_rp::request::PublicKeyCredentialDescriptor {
                        id: cred_id,
                        transports: AuthTransports::NONE,
                    })
                })
                .collect();

        let username = webauthn_rp::request::register::Username::try_from(email.to_string())
            .map_err(|_| ServerCoreError::invalid_input("Invalid username"))?;
        let display_name = webauthn_rp::request::register::Nickname::try_from(email.to_string())
            .map_err(|_| ServerCoreError::invalid_input("Invalid display name"))?;

        let entity = webauthn_rp::request::register::PublicKeyCredentialUserEntity {
            name: username,
            id: &user_handle,
            display_name: Some(display_name),
        };

        let (server_state, client_state) =
            PublicKeyCredentialCreationOptions::passkey(&rp_id, entity, exclude_creds)
                .start_ceremony()
                .map_err(|e| {
                    ServerCoreError::internal(format!("WebAuthn ceremony error: {e:?}"))
                })?;

        // Serialize server state for storage
        let state_bytes = server_state.encode().map_err(enc_err)?;
        let stored_state = StoredChallengeState {
            state: B64.encode(&state_bytes),
        };

        // Store the user_handle alongside the challenge so we can associate it
        // with the credential after verification.
        let user_handle_b64 = B64.encode(user_handle.encode().map_err(enc_err)?);

        let challenge_id = uuid::Uuid::new_v4().to_string();
        let expires_at = chrono::Utc::now().timestamp() + 300; // 5 minutes

        // Pack state + user_handle into the stored JSON
        let full_state = serde_json::json!({
            "ceremony": stored_state,
            "user_handle": user_handle_b64,
        });

        self.store
            .store_challenge(
                &challenge_id,
                Some(user_id),
                email,
                "registration",
                &full_state.to_string(),
                expires_at,
            )
            .await?;

        // Serialize client state to JSON for the browser
        let options = serde_json::to_value(&client_state)
            .map_err(|e| ServerCoreError::internal(e.to_string()))?;

        Ok((challenge_id, options))
    }

    /// Complete passkey registration.
    /// `credential_json` is the raw JSON bytes from the client's credential response.
    pub async fn finish_registration(
        &self,
        challenge_id: &str,
        user_id: &str,
        name: &str,
        credential_json: &[u8],
    ) -> Result<String, ServerCoreError> {
        let rp_id = self.rp_id()?;

        let challenge = self
            .store
            .get_challenge(challenge_id)
            .await?
            .ok_or_else(|| ServerCoreError::not_found("Challenge expired or not found"))?;

        if challenge.user_id.as_deref() != Some(user_id) {
            return Err(ServerCoreError::permission_denied("Challenge mismatch"));
        }

        let full_state: serde_json::Value = serde_json::from_str(&challenge.state_json)
            .map_err(|e| ServerCoreError::internal(format!("Invalid challenge state: {e}")))?;

        let stored_state: StoredChallengeState =
            serde_json::from_value(full_state["ceremony"].clone())
                .map_err(|e| ServerCoreError::internal(format!("Invalid ceremony state: {e}")))?;

        let user_handle_b64 = full_state["user_handle"]
            .as_str()
            .ok_or_else(|| ServerCoreError::internal("Missing user_handle in state"))?;

        let state_bytes = B64.decode(&stored_state.state).map_err(dec_err)?;
        let server_state: RegistrationServerState<USER_HANDLE_MAX_LEN> =
            RegistrationServerState::decode(state_bytes.as_slice()).map_err(dec_err)?;

        // Parse the client's registration response
        let registration = Registration::from_json_custom(credential_json)
            .map_err(|e| ServerCoreError::invalid_input(format!("Invalid credential: {e}")))?;

        // Verify the registration ceremony
        let opts =
            webauthn_rp::request::register::RegistrationVerificationOptions::<&str, &str>::default(
            );
        let registered_cred = server_state
            .verify(&rp_id, &registration, &opts)
            .map_err(|e| {
                ServerCoreError::invalid_input(format!("Registration verification failed: {e}"))
            })?;

        // Serialize the credential for storage
        let cred_id = registered_cred.id();
        let cred_id_bytes = cred_id.encode().map_err(enc_err)?;
        let static_state_bytes = registered_cred.static_state().encode().map_err(enc_err)?;
        let dynamic_state_bytes = registered_cred.dynamic_state().encode().map_err(enc_err)?;

        let stored = StoredCredential {
            cred_id: B64.encode(&cred_id_bytes),
            user_handle: user_handle_b64.to_string(),
            static_state: B64.encode(&static_state_bytes),
            dynamic_state: B64.encode(&dynamic_state_bytes),
        };

        let stored_json =
            serde_json::to_string(&stored).map_err(|e| ServerCoreError::internal(e.to_string()))?;

        let id = self
            .store
            .store_credential(user_id, name, &stored_json)
            .await?;

        info!("Passkey '{}' registered for user {}", name, user_id);
        Ok(id)
    }

    // ========================================================================
    // Authentication
    // ========================================================================

    /// Start passkey authentication (discoverable — no email needed).
    /// Returns (challenge_id, options_json) to send to the client.
    pub async fn start_authentication(
        &self,
        email: Option<&str>,
    ) -> Result<(String, serde_json::Value), ServerCoreError> {
        let rp_id = self.rp_id()?;

        let (server_state, client_state) = DiscoverableCredentialRequestOptions::passkey(&rp_id)
            .start_ceremony()
            .map_err(|e| ServerCoreError::internal(format!("WebAuthn ceremony error: {e:?}")))?;

        let state_bytes = server_state.encode().map_err(enc_err)?;
        let stored_state = StoredChallengeState {
            state: B64.encode(&state_bytes),
        };

        let challenge_id = uuid::Uuid::new_v4().to_string();
        let expires_at = chrono::Utc::now().timestamp() + 300;

        self.store
            .store_challenge(
                &challenge_id,
                None,
                email.unwrap_or(""),
                "authentication",
                &serde_json::to_string(&stored_state)
                    .map_err(|e| ServerCoreError::internal(e.to_string()))?,
                expires_at,
            )
            .await?;

        let options = serde_json::to_value(&client_state)
            .map_err(|e| ServerCoreError::internal(e.to_string()))?;

        Ok((challenge_id, options))
    }

    /// Complete passkey authentication.
    /// `credential_json` is the raw JSON bytes from the client's credential response.
    /// Returns the user_id and email of the authenticated user.
    pub async fn finish_authentication(
        &self,
        challenge_id: &str,
        credential_json: &[u8],
    ) -> Result<PasskeyAuthResult, ServerCoreError> {
        let rp_id = self.rp_id()?;

        let challenge = self
            .store
            .get_challenge(challenge_id)
            .await?
            .ok_or_else(|| ServerCoreError::not_found("Challenge expired or not found"))?;

        let stored_state: StoredChallengeState = serde_json::from_str(&challenge.state_json)
            .map_err(|e| ServerCoreError::internal(format!("Invalid challenge state: {e}")))?;

        let state_bytes = B64.decode(&stored_state.state).map_err(dec_err)?;
        let server_state = DiscoverableAuthenticationServerState::decode(state_bytes.as_slice())
            .map_err(dec_err)?;

        // Parse the client's authentication response
        let authentication = DiscoverableAuthentication64::from_json_custom(credential_json)
            .map_err(|e| {
                ServerCoreError::invalid_input(format!("Invalid authentication response: {e}"))
            })?;

        // Find the matching credential.
        // The authentication response contains raw_id (credential ID) and
        // user_handle (which we stored during registration).
        let raw_cred_id: CredentialId<Vec<u8>> = CredentialId::from(authentication.raw_id());
        let cred_id_encoded = B64.encode(&raw_cred_id.encode().map_err(enc_err)?);

        // Search by email if available, otherwise search all credentials for the user_handle
        let (user_id, stored_cred, cred_db_id) = if !challenge.email.is_empty() {
            self.find_credential_by_email(&challenge.email, &cred_id_encoded)
                .await?
        } else {
            // Discoverable auth: user_handle from the response identifies the user
            let user_handle = authentication.response().user_handle();
            let user_handle_b64 = B64.encode(user_handle.encode().map_err(enc_err)?);
            self.find_credential_by_user_handle(&user_handle_b64, &cred_id_encoded)
                .await?
        };

        // Reconstruct the AuthenticatedCredential
        let static_state_bytes = B64.decode(&stored_cred.static_state).map_err(dec_err)?;
        let dynamic_state_bytes = B64.decode(&stored_cred.dynamic_state).map_err(dec_err)?;
        let stored_cred_id_bytes = B64.decode(&stored_cred.cred_id).map_err(dec_err)?;
        let user_handle_bytes = B64.decode(&stored_cred.user_handle).map_err(dec_err)?;

        let static_state =
            StaticState::<PubKey>::decode(static_state_bytes.as_slice()).map_err(dec_err)?;
        let dynamic_state = DynamicState::decode(
            <[u8; 7]>::try_from(dynamic_state_bytes.as_slice())
                .map_err(|_| ServerCoreError::internal("Invalid dynamic state length"))?,
        )
        .map_err(dec_err)?;
        let stored_cred_id =
            CredentialId::<Vec<u8>>::decode(stored_cred_id_bytes).map_err(dec_err)?;
        let user_handle = UserHandle64::decode(
            user_handle_bytes
                .as_slice()
                .try_into()
                .map_err(|_| ServerCoreError::internal("Invalid user handle length"))?,
        )
        .map_err(dec_err)?;

        let cred_id_ref: CredentialId<&[u8]> = CredentialId::from(&stored_cred_id);
        let mut auth_cred =
            AuthenticatedCredential::new(cred_id_ref, &user_handle, static_state, dynamic_state)
                .map_err(|e| {
                    ServerCoreError::internal(format!(
                        "Failed to construct AuthenticatedCredential: {e}"
                    ))
                })?;

        let opts =
            webauthn_rp::request::auth::AuthenticationVerificationOptions::<&str, &str>::default();

        let updated = server_state
            .verify(&rp_id, &authentication, &mut auth_cred, &opts)
            .map_err(|e| {
                ServerCoreError::invalid_input(format!("Authentication verification failed: {e}"))
            })?;

        // Update dynamic state if changed
        if updated {
            let new_dynamic_bytes = auth_cred.dynamic_state().encode().map_err(enc_err)?;
            let mut updated_stored = stored_cred;
            updated_stored.dynamic_state = B64.encode(&new_dynamic_bytes);
            let updated_json = serde_json::to_string(&updated_stored)
                .map_err(|e| ServerCoreError::internal(e.to_string()))?;
            self.store
                .update_credential(&cred_db_id, &updated_json)
                .await?;
        }

        info!("User {} authenticated via passkey", user_id);
        Ok(PasskeyAuthResult {
            user_id,
            email: challenge.email,
        })
    }

    /// Find a credential by email + credential ID.
    async fn find_credential_by_email(
        &self,
        email: &str,
        cred_id_encoded: &str,
    ) -> Result<(String, StoredCredential, String), ServerCoreError> {
        let creds = self.store.get_credentials_by_email(email).await?;
        for c in creds {
            if let Ok(sc) = serde_json::from_str::<StoredCredential>(&c.credential_json) {
                if sc.cred_id == cred_id_encoded {
                    return Ok((c.user_id, sc, c.id));
                }
            }
        }
        Err(ServerCoreError::not_found("Credential not found"))
    }

    /// Find a credential by user_handle + credential ID.
    async fn find_credential_by_user_handle(
        &self,
        user_handle_b64: &str,
        cred_id_encoded: &str,
    ) -> Result<(String, StoredCredential, String), ServerCoreError> {
        // We need to find which user has this user_handle stored in their credentials.
        // This is a limitation of the current PasskeyStore trait — ideally we'd have
        // a lookup by user_handle. For now, we search by credential content.
        // TODO: Add a get_credentials_by_user_handle method to PasskeyStore.
        let _ = user_handle_b64;
        let _ = cred_id_encoded;
        Err(ServerCoreError::not_found(
            "Discoverable authentication without email is not yet supported. Provide an email.",
        ))
    }

    // ========================================================================
    // CRUD
    // ========================================================================

    /// List passkeys for a user (for UI display).
    pub async fn list_passkeys(&self, user_id: &str) -> Result<Vec<PasskeyInfo>, ServerCoreError> {
        let creds = self.store.get_credentials(user_id).await?;
        Ok(creds
            .into_iter()
            .map(|c| PasskeyInfo {
                id: c.id,
                name: c.name,
                created_at: c.created_at,
                last_used_at: c.last_used_at,
            })
            .collect())
    }

    /// Delete a passkey.
    pub async fn delete_passkey(&self, id: &str, user_id: &str) -> Result<bool, ServerCoreError> {
        self.store.delete_credential(id, user_id).await
    }
}
