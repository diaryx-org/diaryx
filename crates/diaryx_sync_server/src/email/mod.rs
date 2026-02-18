use crate::config::Config;
use reqwest::Client;
use serde::Serialize;
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
