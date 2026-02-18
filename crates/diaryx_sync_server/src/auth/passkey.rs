use crate::config::Config;
use crate::db::AuthRepo;
use crate::db::{PasskeyChallengeInfo, UserInfo};
use chrono::{Duration, Utc};
use std::sync::Arc;
use url::Url;
use webauthn_rs::Webauthn;
use webauthn_rs::prelude::*;
use webauthn_rs_proto::{
    CreationChallengeResponse, PublicKeyCredential, RegisterPublicKeyCredential,
    RequestChallengeResponse,
};

use super::magic_link::{MagicLinkService, VerifyResult};

/// Passkey authentication service.
pub struct PasskeyService {
    webauthn: Webauthn,
    repo: Arc<AuthRepo>,
    magic_link_service: Arc<MagicLinkService>,
}

/// Error types for passkey operations.
#[derive(Debug)]
pub enum PasskeyError {
    NoPasskeys,
    ChallengeNotFound,
    UserNotFound,
    InvalidCredential(String),
    DatabaseError(String),
    WebauthnError(String),
    SessionError(String),
}

impl std::fmt::Display for PasskeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PasskeyError::NoPasskeys => write!(f, "No passkeys registered for this account"),
            PasskeyError::ChallengeNotFound => write!(f, "Challenge not found or expired"),
            PasskeyError::UserNotFound => write!(f, "User not found"),
            PasskeyError::InvalidCredential(e) => write!(f, "Invalid credential: {}", e),
            PasskeyError::DatabaseError(e) => write!(f, "Database error: {}", e),
            PasskeyError::WebauthnError(e) => write!(f, "WebAuthn error: {}", e),
            PasskeyError::SessionError(e) => write!(f, "Session error: {}", e),
        }
    }
}

impl std::error::Error for PasskeyError {}

/// Info about a registered passkey (for listing in UI).
#[derive(Debug, serde::Serialize)]
pub struct PasskeyInfo {
    pub id: String,
    pub name: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

impl PasskeyService {
    pub fn new(
        repo: Arc<AuthRepo>,
        config: Arc<Config>,
        magic_link_service: Arc<MagicLinkService>,
    ) -> Self {
        let app_url = Url::parse(&config.app_base_url)
            .unwrap_or_else(|_| Url::parse("https://app.diaryx.org").unwrap());

        let rp_id = app_url.host_str().unwrap_or("app.diaryx.org").to_string();
        let rp_origin = app_url;

        let builder = WebauthnBuilder::new(&rp_id, &rp_origin)
            .expect("Failed to create WebauthnBuilder")
            .rp_name("Diaryx");

        let webauthn = builder.build().expect("Failed to build Webauthn");

        Self {
            webauthn,
            repo,
            magic_link_service,
        }
    }

    /// Start passkey registration for an authenticated user.
    pub fn start_registration(
        &self,
        user_id: &str,
        email: &str,
    ) -> Result<(CreationChallengeResponse, String), PasskeyError> {
        // Get existing credentials to exclude
        let existing = self
            .repo
            .get_passkey_credentials(user_id)
            .map_err(|e| PasskeyError::DatabaseError(e.to_string()))?;

        let existing_creds: Vec<Passkey> = existing
            .iter()
            .filter_map(|c| serde_json::from_str(&c.credential_json).ok())
            .collect();

        let exclude_creds: Vec<CredentialID> =
            existing_creds.iter().map(|p| p.cred_id().clone()).collect();

        let user_unique_id = Uuid::parse_str(user_id).unwrap_or_else(|_| {
            // Use a deterministic UUID from the user_id string
            Uuid::new_v5(&Uuid::NAMESPACE_URL, user_id.as_bytes())
        });

        let (ccr, reg_state) = self
            .webauthn
            .start_passkey_registration(user_unique_id, email, email, Some(exclude_creds))
            .map_err(|e| PasskeyError::WebauthnError(e.to_string()))?;

        // Store challenge state
        let challenge_id = uuid::Uuid::new_v4().to_string();
        let state_json = serde_json::to_string(&reg_state)
            .map_err(|e| PasskeyError::WebauthnError(e.to_string()))?;
        let expires_at = (Utc::now() + Duration::minutes(5)).timestamp();

        self.repo
            .store_passkey_challenge(
                &challenge_id,
                Some(user_id),
                email,
                "registration",
                &state_json,
                expires_at,
            )
            .map_err(|e| PasskeyError::DatabaseError(e.to_string()))?;

        Ok((ccr, challenge_id))
    }

    /// Complete passkey registration.
    pub fn finish_registration(
        &self,
        challenge_id: &str,
        user_id: &str,
        name: &str,
        credential: &RegisterPublicKeyCredential,
    ) -> Result<String, PasskeyError> {
        let challenge = self
            .repo
            .get_passkey_challenge(challenge_id)
            .map_err(|e| PasskeyError::DatabaseError(e.to_string()))?
            .ok_or(PasskeyError::ChallengeNotFound)?;

        if challenge.user_id.as_deref() != Some(user_id) {
            return Err(PasskeyError::ChallengeNotFound);
        }

        let reg_state: PasskeyRegistration = serde_json::from_str(&challenge.state_json)
            .map_err(|e| PasskeyError::WebauthnError(e.to_string()))?;

        let passkey = self
            .webauthn
            .finish_passkey_registration(credential, &reg_state)
            .map_err(|e| PasskeyError::InvalidCredential(e.to_string()))?;

        let credential_json = serde_json::to_string(&passkey)
            .map_err(|e| PasskeyError::WebauthnError(e.to_string()))?;

        let id = self
            .repo
            .store_passkey_credential(user_id, name, &credential_json)
            .map_err(|e| PasskeyError::DatabaseError(e.to_string()))?;

        Ok(id)
    }

    /// Start passkey authentication for an email.
    pub fn start_authentication(
        &self,
        email: &str,
    ) -> Result<(RequestChallengeResponse, String), PasskeyError> {
        let creds = self
            .repo
            .get_passkey_credentials_by_email(email)
            .map_err(|e| PasskeyError::DatabaseError(e.to_string()))?;

        if creds.is_empty() {
            return Err(PasskeyError::NoPasskeys);
        }

        let passkeys: Vec<Passkey> = creds
            .iter()
            .filter_map(|c| serde_json::from_str(&c.credential_json).ok())
            .collect();

        if passkeys.is_empty() {
            return Err(PasskeyError::NoPasskeys);
        }

        let (rcr, auth_state) = self
            .webauthn
            .start_passkey_authentication(&passkeys)
            .map_err(|e| PasskeyError::WebauthnError(e.to_string()))?;

        let challenge_id = uuid::Uuid::new_v4().to_string();
        let state_json = serde_json::to_string(&auth_state)
            .map_err(|e| PasskeyError::WebauthnError(e.to_string()))?;
        let expires_at = (Utc::now() + Duration::minutes(5)).timestamp();

        // Store user_id as None for auth challenges (resolved on finish)
        self.repo
            .store_passkey_challenge(
                &challenge_id,
                None,
                email,
                "authentication",
                &state_json,
                expires_at,
            )
            .map_err(|e| PasskeyError::DatabaseError(e.to_string()))?;

        Ok((rcr, challenge_id))
    }

    /// Complete passkey authentication — produces a session.
    pub fn finish_authentication(
        &self,
        challenge_id: &str,
        credential: &PublicKeyCredential,
        device_name: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<VerifyResult, PasskeyError> {
        let challenge = self
            .repo
            .get_passkey_challenge(challenge_id)
            .map_err(|e| PasskeyError::DatabaseError(e.to_string()))?
            .ok_or(PasskeyError::ChallengeNotFound)?;

        self.finish_authentication_with_challenge(challenge, credential, device_name, user_agent)
    }

    /// Internal: complete email-scoped passkey auth given an already-consumed challenge.
    fn finish_authentication_with_challenge(
        &self,
        challenge: PasskeyChallengeInfo,
        credential: &PublicKeyCredential,
        device_name: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<VerifyResult, PasskeyError> {
        let auth_state: PasskeyAuthentication = serde_json::from_str(&challenge.state_json)
            .map_err(|e| PasskeyError::WebauthnError(e.to_string()))?;

        let auth_result = self
            .webauthn
            .finish_passkey_authentication(credential, &auth_state)
            .map_err(|e| PasskeyError::InvalidCredential(e.to_string()))?;

        // Update the credential's counter and last_used_at
        let creds = self
            .repo
            .get_passkey_credentials_by_email(&challenge.email)
            .map_err(|e| PasskeyError::DatabaseError(e.to_string()))?;

        for cred_info in &creds {
            if let Ok(mut passkey) = serde_json::from_str::<Passkey>(&cred_info.credential_json) {
                if passkey.cred_id() == auth_result.cred_id() {
                    passkey.update_credential(&auth_result);
                    if let Ok(updated_json) = serde_json::to_string(&passkey) {
                        let _ = self
                            .repo
                            .update_passkey_credential(&cred_info.id, &updated_json);
                    }
                    break;
                }
            }
        }

        // Create session via the shared helper
        self.magic_link_service
            .create_session_for_email(&challenge.email, device_name, user_agent)
            .map_err(|e| PasskeyError::SessionError(e.to_string()))
    }

    /// List passkeys for a user (for UI display).
    pub fn list_passkeys(&self, user_id: &str) -> Result<Vec<PasskeyInfo>, PasskeyError> {
        let creds = self
            .repo
            .get_passkey_credentials(user_id)
            .map_err(|e| PasskeyError::DatabaseError(e.to_string()))?;

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
    pub fn delete_passkey(&self, id: &str, user_id: &str) -> Result<bool, PasskeyError> {
        self.repo
            .delete_passkey_credential(id, user_id)
            .map_err(|e| PasskeyError::DatabaseError(e.to_string()))
    }

    /// Start discoverable authentication (no email required).
    /// The browser will prompt the user to pick from any passkey registered for this origin.
    pub fn start_discoverable_authentication(
        &self,
    ) -> Result<(RequestChallengeResponse, String), PasskeyError> {
        let (rcr, auth_state) = self
            .webauthn
            .start_discoverable_authentication()
            .map_err(|e| PasskeyError::WebauthnError(e.to_string()))?;

        let challenge_id = uuid::Uuid::new_v4().to_string();
        let state_json = serde_json::to_string(&auth_state)
            .map_err(|e| PasskeyError::WebauthnError(e.to_string()))?;
        let expires_at = (Utc::now() + Duration::minutes(5)).timestamp();

        self.repo
            .store_passkey_challenge(
                &challenge_id,
                None,
                "",
                "discoverable_authentication",
                &state_json,
                expires_at,
            )
            .map_err(|e| PasskeyError::DatabaseError(e.to_string()))?;

        Ok((rcr, challenge_id))
    }

    /// Complete discoverable authentication — identifies the user from the credential.
    pub fn finish_discoverable_authentication(
        &self,
        challenge_id: &str,
        credential: &PublicKeyCredential,
        device_name: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<VerifyResult, PasskeyError> {
        let challenge = self
            .repo
            .get_passkey_challenge(challenge_id)
            .map_err(|e| PasskeyError::DatabaseError(e.to_string()))?
            .ok_or(PasskeyError::ChallengeNotFound)?;

        self.finish_discoverable_with_challenge(challenge, credential, device_name, user_agent)
    }

    /// Internal: complete discoverable auth given an already-consumed challenge.
    fn finish_discoverable_with_challenge(
        &self,
        challenge: PasskeyChallengeInfo,
        credential: &PublicKeyCredential,
        device_name: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<VerifyResult, PasskeyError> {
        let auth_state: DiscoverableAuthentication = serde_json::from_str(&challenge.state_json)
            .map_err(|e| PasskeyError::WebauthnError(e.to_string()))?;

        // Identify which user owns this credential
        let (user_uuid, _cred_id_bytes) = self
            .webauthn
            .identify_discoverable_authentication(credential)
            .map_err(|e| PasskeyError::InvalidCredential(e.to_string()))?;

        let user_id = user_uuid.to_string();

        // Look up user to get email
        let user: UserInfo = self
            .repo
            .get_user(&user_id)
            .map_err(|e| PasskeyError::DatabaseError(e.to_string()))?
            .ok_or(PasskeyError::UserNotFound)?;

        // Get all passkeys for this user and convert to DiscoverableKey
        let creds = self
            .repo
            .get_passkey_credentials(&user_id)
            .map_err(|e| PasskeyError::DatabaseError(e.to_string()))?;

        let discoverable_keys: Vec<DiscoverableKey> = creds
            .iter()
            .filter_map(|c| serde_json::from_str::<Passkey>(&c.credential_json).ok())
            .map(DiscoverableKey::from)
            .collect();

        if discoverable_keys.is_empty() {
            return Err(PasskeyError::NoPasskeys);
        }

        let auth_result = self
            .webauthn
            .finish_discoverable_authentication(credential, auth_state, &discoverable_keys)
            .map_err(|e| PasskeyError::InvalidCredential(e.to_string()))?;

        // Update the credential's counter and last_used_at
        for cred_info in &creds {
            if let Ok(mut passkey) = serde_json::from_str::<Passkey>(&cred_info.credential_json) {
                if passkey.cred_id() == auth_result.cred_id() {
                    passkey.update_credential(&auth_result);
                    if let Ok(updated_json) = serde_json::to_string(&passkey) {
                        let _ = self
                            .repo
                            .update_passkey_credential(&cred_info.id, &updated_json);
                    }
                    break;
                }
            }
        }

        // Create session via the shared helper
        self.magic_link_service
            .create_session_for_email(&user.email, device_name, user_agent)
            .map_err(|e| PasskeyError::SessionError(e.to_string()))
    }

    /// Unified finish that dispatches based on the challenge type stored in DB.
    pub fn finish_any_authentication(
        &self,
        challenge_id: &str,
        credential: &PublicKeyCredential,
        device_name: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<VerifyResult, PasskeyError> {
        let challenge = self
            .repo
            .get_passkey_challenge(challenge_id)
            .map_err(|e| PasskeyError::DatabaseError(e.to_string()))?
            .ok_or(PasskeyError::ChallengeNotFound)?;

        match challenge.challenge_type.as_str() {
            "authentication" => self.finish_authentication_with_challenge(
                challenge,
                credential,
                device_name,
                user_agent,
            ),
            "discoverable_authentication" => self.finish_discoverable_with_challenge(
                challenge,
                credential,
                device_name,
                user_agent,
            ),
            _ => Err(PasskeyError::ChallengeNotFound),
        }
    }
}
