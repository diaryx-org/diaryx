//! Passkey API request/response types.

use serde::{Deserialize, Serialize};

/// Response from POST /auth/passkeys/register/start
#[derive(Debug, Serialize, Deserialize)]
pub struct PasskeyRegisterStartResponse {
    pub challenge_id: String,
    pub options: serde_json::Value,
}

/// POST /auth/passkeys/register/finish
#[derive(Debug, Deserialize)]
pub struct PasskeyRegisterFinishRequest {
    pub challenge_id: String,
    pub name: String,
    pub credential: serde_json::Value,
}

/// Response from POST /auth/passkeys/register/finish
#[derive(Debug, Serialize, Deserialize)]
pub struct PasskeyRegisterFinishResponse {
    pub id: String,
}

/// POST /auth/passkeys/authenticate/start
#[derive(Debug, Deserialize)]
pub struct PasskeyAuthStartRequest {
    pub email: Option<String>,
}

/// Response from POST /auth/passkeys/authenticate/start
#[derive(Debug, Serialize, Deserialize)]
pub struct PasskeyAuthStartResponse {
    pub challenge_id: String,
    pub options: serde_json::Value,
}

/// POST /auth/passkeys/authenticate/finish
#[derive(Debug, Deserialize)]
pub struct PasskeyAuthFinishRequest {
    pub challenge_id: String,
    pub credential: serde_json::Value,
    pub device_name: Option<String>,
    pub replace_device_id: Option<String>,
}

/// Item in passkey list response.
#[derive(Debug, Serialize, Deserialize)]
pub struct PasskeyListItem {
    pub id: String,
    pub name: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}
