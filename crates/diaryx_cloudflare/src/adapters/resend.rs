//! Resend email adapter using the Workers Fetch API.
//!
//! Same Resend HTTP API as the native server, but uses `worker::Fetch`
//! instead of `reqwest` since reqwest doesn't work in Workers.

use async_trait::async_trait;
use diaryx_server::ports::{Mailer, ServerCoreError};
use serde::Serialize;
use worker::{Fetch, Headers, Method, Request, RequestInit};

fn e(err: impl std::fmt::Display) -> ServerCoreError {
    ServerCoreError::internal(err.to_string())
}

pub struct ResendMailer {
    api_key: String,
    from_name: String,
    from_email: String,
    magic_link_expiry_minutes: i64,
}

impl ResendMailer {
    pub fn new(
        api_key: String,
        from_name: String,
        from_email: String,
        magic_link_expiry_minutes: i64,
    ) -> Self {
        Self {
            api_key,
            from_name,
            from_email,
            magic_link_expiry_minutes,
        }
    }

    fn build_email_body(&self, magic_link_url: &str, verification_code: &str) -> String {
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
            expiry = self.magic_link_expiry_minutes,
            code = spaced_code,
            link = magic_link_url,
        )
    }
}

#[derive(Serialize)]
struct ResendRequest {
    from: String,
    to: Vec<String>,
    subject: String,
    html: String,
}

#[async_trait(?Send)]
impl Mailer for ResendMailer {
    async fn send_magic_link(
        &self,
        to_email: &str,
        magic_link_url: &str,
        verification_code: &str,
    ) -> Result<(), ServerCoreError> {
        let body = ResendRequest {
            from: format!("{} <{}>", self.from_name, self.from_email),
            to: vec![to_email.to_string()],
            subject: "Sign in to Diaryx".to_string(),
            html: self.build_email_body(magic_link_url, verification_code),
        };

        let json = serde_json::to_string(&body).map_err(e)?;

        let headers = Headers::new();
        headers
            .set("Authorization", &format!("Bearer {}", self.api_key))
            .map_err(e)?;
        headers.set("Content-Type", "application/json").map_err(e)?;

        let mut init = RequestInit::new();
        init.with_method(Method::Post);
        init.with_headers(headers);
        init.with_body(Some(json.into()));

        let req = Request::new_with_init("https://api.resend.com/emails", &init).map_err(e)?;
        let mut resp = Fetch::Request(req).send().await.map_err(e)?;

        if resp.status_code() >= 400 {
            let text = resp.text().await.unwrap_or_default();
            return Err(ServerCoreError::internal(format!(
                "Resend API error {}: {}",
                resp.status_code(),
                text
            )));
        }

        Ok(())
    }
}
