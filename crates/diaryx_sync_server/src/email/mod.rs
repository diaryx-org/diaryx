use crate::config::Config;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{error, info};

/// Email service using Resend HTTP API
pub struct EmailService {
    config: Arc<Config>,
    client: Option<Client>,
}

#[derive(Serialize)]
struct ResendRequest {
    from: String,
    to: Vec<String>,
    subject: String,
    html: String,
}

/// Error types for email operations
#[derive(Debug)]
pub enum EmailError {
    /// Email service not configured
    NotConfigured,
    /// Failed to send email
    SendError(String),
}

impl std::fmt::Display for EmailError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EmailError::NotConfigured => write!(f, "Email service not configured"),
            EmailError::SendError(e) => write!(f, "Failed to send email: {}", e),
        }
    }
}

impl std::error::Error for EmailError {}

// ============================================================================
// Resend API types
// ============================================================================

#[derive(Debug, Deserialize)]
struct ResendAudienceResponse {
    id: String,
}

#[derive(Debug, Deserialize)]
struct ResendContactResponse {
    id: String,
}

#[derive(Debug, Deserialize)]
struct ResendContactListResponse {
    data: Vec<ResendContact>,
}

/// A contact in a Resend audience.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResendContact {
    pub id: String,
    pub email: String,
    #[serde(default)]
    pub unsubscribed: bool,
    pub created_at: Option<String>,
}

/// A single email in a Resend batch send request.
#[derive(Debug, Serialize)]
pub struct ResendBatchEmail {
    pub from: String,
    pub to: Vec<String>,
    pub subject: String,
    pub html: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<std::collections::HashMap<String, String>>,
}

impl EmailService {
    /// Create a new EmailService
    pub fn new(config: Arc<Config>) -> Self {
        let client = if config.is_email_configured() {
            info!("Email service configured with Resend API");
            Some(Client::new())
        } else {
            info!("Email service not configured (RESEND_API_KEY missing)");
            None
        };

        Self { config, client }
    }

    /// Check if email service is configured
    pub fn is_configured(&self) -> bool {
        self.client.is_some()
    }

    /// Get the configured "from" display name.
    pub fn from_name(&self) -> &str {
        &self.config.email.from_name
    }

    /// Get the configured "from" email address.
    pub fn from_email(&self) -> &str {
        &self.config.email.from_email
    }

    /// Send a magic link email with a verification code
    pub async fn send_magic_link(
        &self,
        to_email: &str,
        magic_link_url: &str,
        verification_code: &str,
    ) -> Result<(), EmailError> {
        let client = self.client.as_ref().ok_or(EmailError::NotConfigured)?;

        let body = ResendRequest {
            from: format!(
                "{} <{}>",
                self.config.email.from_name, self.config.email.from_email
            ),
            to: vec![to_email.to_string()],
            subject: "Sign in to Diaryx".to_string(),
            html: self.build_magic_link_email_body(magic_link_url, verification_code),
        };

        let resp = client
            .post("https://api.resend.com/emails")
            .bearer_auth(&self.config.email.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| EmailError::SendError(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            error!("Resend API error: {} - {}", status, text);
            return Err(EmailError::SendError(format!("{}: {}", status, text)));
        }

        info!("Magic link email sent to {}", to_email);
        Ok(())
    }

    // ========================================================================
    // Resend Audiences API
    // ========================================================================

    /// Create a Resend audience. Returns the audience ID.
    pub async fn create_audience(&self, name: &str) -> Result<String, EmailError> {
        let client = self.client.as_ref().ok_or(EmailError::NotConfigured)?;

        let resp = client
            .post("https://api.resend.com/audiences")
            .bearer_auth(&self.config.email.api_key)
            .json(&serde_json::json!({ "name": name }))
            .send()
            .await
            .map_err(|e| EmailError::SendError(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            error!("Resend create audience error: {} - {}", status, text);
            return Err(EmailError::SendError(format!("{}: {}", status, text)));
        }

        let body: ResendAudienceResponse = resp
            .json()
            .await
            .map_err(|e| EmailError::SendError(e.to_string()))?;

        info!("Created Resend audience '{}' with id {}", name, body.id);
        Ok(body.id)
    }

    /// Delete a Resend audience.
    pub async fn delete_audience(&self, audience_id: &str) -> Result<(), EmailError> {
        let client = self.client.as_ref().ok_or(EmailError::NotConfigured)?;

        let resp = client
            .delete(format!("https://api.resend.com/audiences/{}", audience_id))
            .bearer_auth(&self.config.email.api_key)
            .send()
            .await
            .map_err(|e| EmailError::SendError(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            error!("Resend delete audience error: {} - {}", status, text);
            return Err(EmailError::SendError(format!("{}: {}", status, text)));
        }

        info!("Deleted Resend audience {}", audience_id);
        Ok(())
    }

    /// Add a contact to a Resend audience. Returns the contact ID.
    pub async fn add_contact(&self, audience_id: &str, email: &str) -> Result<String, EmailError> {
        let client = self.client.as_ref().ok_or(EmailError::NotConfigured)?;

        let resp = client
            .post(format!(
                "https://api.resend.com/audiences/{}/contacts",
                audience_id
            ))
            .bearer_auth(&self.config.email.api_key)
            .json(&serde_json::json!({ "email": email }))
            .send()
            .await
            .map_err(|e| EmailError::SendError(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            error!("Resend add contact error: {} - {}", status, text);
            return Err(EmailError::SendError(format!("{}: {}", status, text)));
        }

        let body: ResendContactResponse = resp
            .json()
            .await
            .map_err(|e| EmailError::SendError(e.to_string()))?;

        info!("Added contact {} to audience {}", email, audience_id);
        Ok(body.id)
    }

    /// Remove a contact from a Resend audience.
    pub async fn remove_contact(
        &self,
        audience_id: &str,
        contact_id: &str,
    ) -> Result<(), EmailError> {
        let client = self.client.as_ref().ok_or(EmailError::NotConfigured)?;

        let resp = client
            .delete(format!(
                "https://api.resend.com/audiences/{}/contacts/{}",
                audience_id, contact_id
            ))
            .bearer_auth(&self.config.email.api_key)
            .send()
            .await
            .map_err(|e| EmailError::SendError(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            error!("Resend remove contact error: {} - {}", status, text);
            return Err(EmailError::SendError(format!("{}: {}", status, text)));
        }

        info!(
            "Removed contact {} from audience {}",
            contact_id, audience_id
        );
        Ok(())
    }

    /// List all contacts in a Resend audience.
    pub async fn list_contacts(&self, audience_id: &str) -> Result<Vec<ResendContact>, EmailError> {
        let client = self.client.as_ref().ok_or(EmailError::NotConfigured)?;

        let resp = client
            .get(format!(
                "https://api.resend.com/audiences/{}/contacts",
                audience_id
            ))
            .bearer_auth(&self.config.email.api_key)
            .send()
            .await
            .map_err(|e| EmailError::SendError(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            error!("Resend list contacts error: {} - {}", status, text);
            return Err(EmailError::SendError(format!("{}: {}", status, text)));
        }

        let body: ResendContactListResponse = resp
            .json()
            .await
            .map_err(|e| EmailError::SendError(e.to_string()))?;

        Ok(body.data)
    }

    /// Send a batch of emails via Resend.
    pub async fn send_batch(&self, emails: Vec<ResendBatchEmail>) -> Result<(), EmailError> {
        let client = self.client.as_ref().ok_or(EmailError::NotConfigured)?;

        let resp = client
            .post("https://api.resend.com/emails/batch")
            .bearer_auth(&self.config.email.api_key)
            .json(&emails)
            .send()
            .await
            .map_err(|e| EmailError::SendError(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            error!("Resend batch send error: {} - {}", status, text);
            return Err(EmailError::SendError(format!("{}: {}", status, text)));
        }

        info!("Sent batch of {} emails", emails.len());
        Ok(())
    }

    // ========================================================================
    // Private helpers
    // ========================================================================

    fn build_magic_link_email_body(&self, magic_link_url: &str, verification_code: &str) -> String {
        // Space out the digits for readability
        let spaced_code: String = verification_code
            .chars()
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
            .join(" ");

        format!(
            r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Sign in to Diaryx</title>
</head>
<body style="font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; line-height: 1.6; color: #333; max-width: 600px; margin: 0 auto; padding: 20px;">
    <div style="text-align: center; margin-bottom: 30px;">
        <h1 style="color: #1a1a1a; margin-bottom: 10px;">Diaryx</h1>
    </div>

    <div style="background-color: #f9f9f9; border-radius: 8px; padding: 30px; margin-bottom: 20px;">
        <h2 style="margin-top: 0; color: #1a1a1a;">Sign in to your account</h2>
        <p>Click the button below to sign in to Diaryx. This link will expire in {expiry} minutes.</p>

        <div style="text-align: center; margin: 24px 0;">
            <p style="color: #666; font-size: 14px; margin-bottom: 8px;">Or enter this code in the app:</p>
            <div style="display: inline-block; background-color: #fff; border: 2px solid #e0e0e0; border-radius: 8px; padding: 12px 24px;">
                <span style="font-family: 'SF Mono', SFMono-Regular, Consolas, 'Liberation Mono', Menlo, monospace; font-size: 28px; letter-spacing: 6px; color: #1a1a1a; font-weight: 600;">{code}</span>
            </div>
        </div>

        <div style="text-align: center; margin: 30px 0;">
            <a href="{link}" style="display: inline-block; background-color: #0066cc; color: white; text-decoration: none; padding: 14px 28px; border-radius: 6px; font-weight: 500;">
                Sign in to Diaryx
            </a>
        </div>

        <p style="color: #666; font-size: 14px;">
            If the button doesn't work, copy and paste this link into your browser:
        </p>
        <p style="word-break: break-all; color: #0066cc; font-size: 14px;">
            <a href="{link}" style="color: #0066cc;">{link}</a>
        </p>
    </div>

    <div style="text-align: center; color: #999; font-size: 12px;">
        <p>If you didn't request this email, you can safely ignore it.</p>
        <p>&copy; Diaryx</p>
    </div>
</body>
</html>"#,
            expiry = self.config.magic_link_expiry_minutes,
            code = spaced_code,
            link = magic_link_url,
        )
    }
}

// ============================================================================
// EmailBroadcastService trait implementation
// ============================================================================

fn email_err(e: impl std::fmt::Display) -> diaryx_server::ports::ServerCoreError {
    diaryx_server::ports::ServerCoreError::internal(e.to_string())
}

#[async_trait::async_trait]
impl diaryx_server::ports::EmailBroadcastService for EmailService {
    fn is_configured(&self) -> bool {
        self.client.is_some()
    }

    fn from_name(&self) -> &str {
        &self.config.email.from_name
    }

    fn from_email(&self) -> &str {
        &self.config.email.from_email
    }

    async fn create_audience(
        &self,
        name: &str,
    ) -> Result<String, diaryx_server::ports::ServerCoreError> {
        self.create_audience(name).await.map_err(email_err)
    }

    async fn delete_audience(
        &self,
        audience_id: &str,
    ) -> Result<(), diaryx_server::ports::ServerCoreError> {
        self.delete_audience(audience_id).await.map_err(email_err)
    }

    async fn add_contact(
        &self,
        audience_id: &str,
        email: &str,
    ) -> Result<String, diaryx_server::ports::ServerCoreError> {
        self.add_contact(audience_id, email)
            .await
            .map_err(email_err)
    }

    async fn remove_contact(
        &self,
        audience_id: &str,
        contact_id: &str,
    ) -> Result<(), diaryx_server::ports::ServerCoreError> {
        self.remove_contact(audience_id, contact_id)
            .await
            .map_err(email_err)
    }

    async fn list_contacts(
        &self,
        audience_id: &str,
    ) -> Result<Vec<diaryx_server::domain::ContactInfo>, diaryx_server::ports::ServerCoreError>
    {
        let contacts = self.list_contacts(audience_id).await.map_err(email_err)?;
        Ok(contacts
            .into_iter()
            .map(|c| diaryx_server::domain::ContactInfo {
                id: c.id,
                email: c.email,
                unsubscribed: c.unsubscribed,
                created_at: c.created_at,
            })
            .collect())
    }

    async fn send_batch(
        &self,
        from: &str,
        emails: Vec<(
            String,
            String,
            String,
            Option<String>,
            Option<std::collections::HashMap<String, String>>,
        )>,
    ) -> Result<(), diaryx_server::ports::ServerCoreError> {
        let batch: Vec<ResendBatchEmail> = emails
            .into_iter()
            .map(|(to, subject, html, reply_to, headers)| ResendBatchEmail {
                from: from.to_string(),
                to: vec![to],
                subject,
                html,
                reply_to,
                headers,
            })
            .collect();
        self.send_batch(batch).await.map_err(email_err)
    }
}
