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

#[cfg(test)]
mod tests {
    use super::{
        PasskeyAuthFinishRequest, PasskeyAuthStartRequest, PasskeyAuthStartResponse,
        PasskeyListItem, PasskeyRegisterFinishRequest, PasskeyRegisterFinishResponse,
        PasskeyRegisterStartResponse,
    };

    #[test]
    fn passkey_requests_deserialize_optional_fields() {
        let register_finish: PasskeyRegisterFinishRequest =
            serde_json::from_value(serde_json::json!({
                "challenge_id": "challenge-1",
                "name": "Laptop",
                "credential": { "id": "cred-1" }
            }))
            .unwrap();
        let auth_start: PasskeyAuthStartRequest = serde_json::from_value(serde_json::json!({
            "email": "user@example.com"
        }))
        .unwrap();
        let auth_finish: PasskeyAuthFinishRequest = serde_json::from_value(serde_json::json!({
            "challenge_id": "challenge-2",
            "credential": { "id": "cred-2" },
            "device_name": "New MacBook",
            "replace_device_id": "device-123"
        }))
        .unwrap();

        assert_eq!(register_finish.challenge_id, "challenge-1");
        assert_eq!(register_finish.name, "Laptop");
        assert_eq!(
            register_finish.credential,
            serde_json::json!({ "id": "cred-1" })
        );
        assert_eq!(auth_start.email.as_deref(), Some("user@example.com"));
        assert_eq!(auth_finish.challenge_id, "challenge-2");
        assert_eq!(
            auth_finish.credential,
            serde_json::json!({ "id": "cred-2" })
        );
        assert_eq!(auth_finish.device_name.as_deref(), Some("New MacBook"));
        assert_eq!(auth_finish.replace_device_id.as_deref(), Some("device-123"));
    }

    #[test]
    fn passkey_responses_serialize_expected_shapes() {
        let register_start = serde_json::to_value(PasskeyRegisterStartResponse {
            challenge_id: "challenge-1".to_string(),
            options: serde_json::json!({ "challenge": "abc" }),
        })
        .unwrap();
        let register_finish = serde_json::to_value(PasskeyRegisterFinishResponse {
            id: "cred-1".to_string(),
        })
        .unwrap();
        let auth_start = serde_json::to_value(PasskeyAuthStartResponse {
            challenge_id: "challenge-2".to_string(),
            options: serde_json::json!({ "challenge": "def" }),
        })
        .unwrap();
        let list_item = serde_json::to_value(PasskeyListItem {
            id: "cred-1".to_string(),
            name: "Laptop".to_string(),
            created_at: 123,
            last_used_at: Some(456),
        })
        .unwrap();

        assert_eq!(
            register_start,
            serde_json::json!({
                "challenge_id": "challenge-1",
                "options": { "challenge": "abc" }
            })
        );
        assert_eq!(register_finish, serde_json::json!({ "id": "cred-1" }));
        assert_eq!(
            auth_start,
            serde_json::json!({
                "challenge_id": "challenge-2",
                "options": { "challenge": "def" }
            })
        );
        assert_eq!(
            list_item,
            serde_json::json!({
                "id": "cred-1",
                "name": "Laptop",
                "created_at": 123,
                "last_used_at": 456
            })
        );
    }
}
