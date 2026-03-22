//! Proxy resolution use case.
//!
//! Resolves a proxy request into a forwarding action or rejection.
//! Transport-agnostic — the caller is responsible for making the actual HTTP request.

use crate::domain::UserTier;
use crate::ports::{ProxyConfigStore, ProxySecretResolver, ProxyUsageStore, ServerCoreError};
use crate::proxy::{ProxyAuthMethod, ProxyForward, ProxyResult, sign_proxy_request};
use std::collections::HashMap;

/// Input for proxy resolution.
pub struct ProxyRequest {
    pub proxy_id: String,
    pub path: String,
    pub method: String,
    pub headers: HashMap<String, String>,
    pub body: Option<Vec<u8>>,
    pub user_id: String,
    pub user_tier: UserTier,
}

pub struct ProxyService<
    'a,
    C: ProxyConfigStore + ?Sized,
    S: ProxySecretResolver + ?Sized,
    U: ProxyUsageStore + ?Sized,
> {
    config_store: &'a C,
    secret_resolver: &'a S,
    usage_store: &'a U,
}

impl<'a, C: ProxyConfigStore, S: ProxySecretResolver, U: ProxyUsageStore>
    ProxyService<'a, C, S, U>
{
    pub fn new(config_store: &'a C, secret_resolver: &'a S, usage_store: &'a U) -> Self {
        Self {
            config_store,
            secret_resolver,
            usage_store,
        }
    }

    pub async fn resolve(&self, request: ProxyRequest) -> Result<ProxyResult, ServerCoreError> {
        // 1. Look up proxy config
        let config = self
            .config_store
            .get_proxy(&request.proxy_id)
            .await?
            .ok_or_else(|| {
                ServerCoreError::NotFound(format!("Proxy '{}' not found", request.proxy_id))
            })?;

        // 2. Check path allowlist
        if let Some(ref allowed) = config.allowed_paths {
            let path_ok = allowed.iter().any(|p| request.path.starts_with(p));
            if !path_ok {
                return Ok(ProxyResult::Rejected {
                    status: 403,
                    code: "path_not_allowed".into(),
                    message: format!("Path '{}' is not allowed for this proxy", request.path),
                });
            }
        }

        // 3. Validate request body
        if let Some(ref validation) = config.validation {
            if let Some(max_bytes) = validation.max_body_bytes {
                let body_len = request.body.as_ref().map(|b| b.len()).unwrap_or(0);
                if body_len > max_bytes {
                    return Ok(ProxyResult::Rejected {
                        status: 413,
                        code: "body_too_large".into(),
                        message: format!("Request body exceeds {} bytes", max_bytes),
                    });
                }
            }

            if !validation.allowed_values.is_empty() {
                if let Some(ref body) = request.body {
                    if let Ok(json) = serde_json::from_slice::<serde_json::Value>(body) {
                        for (field, allowed) in &validation.allowed_values {
                            if let Some(value) = json.get(field).and_then(|v| v.as_str()) {
                                if !allowed.iter().any(|a| a == value) {
                                    return Ok(ProxyResult::Rejected {
                                        status: 400,
                                        code: "value_not_allowed".into(),
                                        message: format!(
                                            "Value '{}' for field '{}' is not in the allowlist",
                                            value, field
                                        ),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        // 4. Check tier (platform secrets only)
        if let ProxyAuthMethod::PlatformSecret { required_tier, .. } = &config.auth_method {
            if request.user_tier != *required_tier && *required_tier == UserTier::Plus {
                return Ok(ProxyResult::Rejected {
                    status: 403,
                    code: "plus_required".into(),
                    message: "Diaryx Plus is required for this proxy".into(),
                });
            }
        }

        // 5. Check rate limit
        if let Some(limit) = config.rate_limit_per_minute {
            let allowed = self
                .usage_store
                .check_rate_limit(&request.user_id, &request.proxy_id, limit)
                .await?;
            if !allowed {
                return Ok(ProxyResult::Rejected {
                    status: 429,
                    code: "rate_limited".into(),
                    message: format!("Rate limit exceeded ({} requests/min)", limit),
                });
            }
        }

        // 6. Check monthly quota
        if let Some(quota) = config.monthly_quota {
            let period = chrono::Utc::now().format("%Y-%m").to_string();
            let count = self
                .usage_store
                .get_monthly_count(&request.user_id, &request.proxy_id, &period)
                .await?;
            if count >= quota {
                return Ok(ProxyResult::Rejected {
                    status: 429,
                    code: "quota_exceeded".into(),
                    message: format!("Monthly quota of {} requests exceeded", quota),
                });
            }
        }

        // 7. Resolve credentials and build upstream request
        let mut upstream_headers = request.headers.clone();
        let url = format!(
            "{}/{}",
            config.upstream.trim_end_matches('/'),
            request.path.trim_start_matches('/')
        );

        match &config.auth_method {
            ProxyAuthMethod::PlatformSecret {
                env_key,
                auth_header,
                auth_prefix,
                ..
            } => {
                let secret = self
                    .secret_resolver
                    .resolve_platform_secret(env_key)
                    .ok_or(ServerCoreError::Internal(format!(
                        "Platform secret '{}' not configured",
                        env_key
                    )))?;
                upstream_headers.insert(auth_header.clone(), format!("{}{}", auth_prefix, secret));
            }
            ProxyAuthMethod::UserSecret {
                secret_key,
                auth_header,
                auth_prefix,
            } => {
                let secret = self
                    .secret_resolver
                    .resolve_user_secret(&request.user_id, secret_key)
                    .await
                    .ok_or(ServerCoreError::InvalidInput(format!(
                        "User secret '{}' not set. Configure your API key first.",
                        secret_key
                    )))?;
                upstream_headers.insert(auth_header.clone(), format!("{}{}", auth_prefix, secret));
            }
            ProxyAuthMethod::HmacSigned { hmac_secret_env } => {
                let hmac_secret = self
                    .secret_resolver
                    .resolve_platform_secret(hmac_secret_env)
                    .ok_or(ServerCoreError::Internal(format!(
                        "HMAC secret '{}' not configured",
                        hmac_secret_env
                    )))?;
                let timestamp = chrono::Utc::now().timestamp() as u64;
                let body_bytes = request.body.as_deref().unwrap_or(&[]);
                let signature = sign_proxy_request(
                    hmac_secret.as_bytes(),
                    timestamp,
                    &request.user_id,
                    body_bytes,
                );

                upstream_headers.insert("X-Diaryx-Timestamp".into(), timestamp.to_string());
                upstream_headers.insert("X-Diaryx-User".into(), request.user_id.clone());
                upstream_headers.insert("X-Diaryx-Signature".into(), signature);
            }
        }

        Ok(ProxyResult::Forward(ProxyForward {
            url,
            headers: upstream_headers,
            body: request.body,
            streaming: config.streaming,
        }))
    }

    /// Record successful proxy usage (call after upstream returns success).
    pub async fn record_usage(&self, user_id: &str, proxy_id: &str) -> Result<(), ServerCoreError> {
        let period = chrono::Utc::now().format("%Y-%m").to_string();
        self.usage_store
            .increment_monthly_count(user_id, proxy_id, &period)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{ProxyRequest, ProxyService};
    use crate::domain::UserTier;
    use crate::ports::{ProxyConfigStore, ProxySecretResolver, ProxyUsageStore, ServerCoreError};
    use crate::proxy::{
        ProxyAuthMethod, ProxyConfig, ProxyResult, ProxyValidation, verify_proxy_signature,
    };
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct TestConfigStore {
        proxies: Mutex<HashMap<String, ProxyConfig>>,
    }

    crate::cfg_async_trait! {
    impl ProxyConfigStore for TestConfigStore {
        async fn get_proxy(&self, proxy_id: &str) -> Result<Option<ProxyConfig>, ServerCoreError> {
            Ok(self.proxies.lock().unwrap().get(proxy_id).cloned())
        }

        async fn list_proxies(&self) -> Result<Vec<ProxyConfig>, ServerCoreError> {
            Ok(self.proxies.lock().unwrap().values().cloned().collect())
        }
    }
    }

    #[derive(Default)]
    struct TestSecretResolver {
        platform_secrets: Mutex<HashMap<String, String>>,
        user_secrets: Mutex<HashMap<(String, String), String>>,
    }

    crate::cfg_async_trait! {
    impl ProxySecretResolver for TestSecretResolver {
        fn resolve_platform_secret(&self, env_key: &str) -> Option<String> {
            self.platform_secrets.lock().unwrap().get(env_key).cloned()
        }

        async fn resolve_user_secret(&self, user_id: &str, secret_key: &str) -> Option<String> {
            self.user_secrets
                .lock()
                .unwrap()
                .get(&(user_id.to_string(), secret_key.to_string()))
                .cloned()
        }
    }
    }

    struct IncrementCall {
        user_id: String,
        proxy_id: String,
        period: String,
    }

    struct TestUsageStore {
        rate_limit_allowed: Mutex<bool>,
        monthly_count: Mutex<u64>,
        increment_calls: Mutex<Vec<IncrementCall>>,
    }

    impl Default for TestUsageStore {
        fn default() -> Self {
            Self {
                rate_limit_allowed: Mutex::new(true),
                monthly_count: Mutex::new(0),
                increment_calls: Mutex::new(vec![]),
            }
        }
    }

    crate::cfg_async_trait! {
    impl ProxyUsageStore for TestUsageStore {
        async fn get_monthly_count(
            &self,
            _: &str,
            _: &str,
            _: &str,
        ) -> Result<u64, ServerCoreError> {
            Ok(*self.monthly_count.lock().unwrap())
        }

        async fn increment_monthly_count(
            &self,
            user_id: &str,
            proxy_id: &str,
            period: &str,
        ) -> Result<u64, ServerCoreError> {
            self.increment_calls.lock().unwrap().push(IncrementCall {
                user_id: user_id.to_string(),
                proxy_id: proxy_id.to_string(),
                period: period.to_string(),
            });
            let mut count = self.monthly_count.lock().unwrap();
            *count += 1;
            Ok(*count)
        }

        async fn check_rate_limit(
            &self,
            _: &str,
            _: &str,
            _: u32,
        ) -> Result<bool, ServerCoreError> {
            Ok(*self.rate_limit_allowed.lock().unwrap())
        }
    }
    }

    fn proxy_config(auth_method: ProxyAuthMethod) -> ProxyConfig {
        ProxyConfig {
            proxy_id: "proxy-1".to_string(),
            upstream: "https://api.example.com/v1/".to_string(),
            auth_method,
            allowed_paths: None,
            rate_limit_per_minute: None,
            monthly_quota: None,
            streaming: false,
            validation: None,
        }
    }

    fn proxy_request() -> ProxyRequest {
        ProxyRequest {
            proxy_id: "proxy-1".to_string(),
            path: "/chat/completions".to_string(),
            method: "POST".to_string(),
            headers: HashMap::from([("X-Client".to_string(), "diaryx-tests".to_string())]),
            body: Some(br#"{"model":"gpt-4o-mini"}"#.to_vec()),
            user_id: "user1".to_string(),
            user_tier: UserTier::Plus,
        }
    }

    fn assert_rejected(result: ProxyResult, expected_status: u16, expected_code: &str) {
        match result {
            ProxyResult::Rejected { status, code, .. } => {
                assert_eq!(status, expected_status);
                assert_eq!(code, expected_code);
            }
            ProxyResult::Forward(_) => panic!("expected rejection"),
        }
    }

    #[tokio::test]
    async fn resolve_returns_not_found_when_proxy_is_missing() {
        let config_store = TestConfigStore::default();
        let resolver = TestSecretResolver::default();
        let usage_store = TestUsageStore::default();
        let service = ProxyService::new(&config_store, &resolver, &usage_store);

        let err = match service.resolve(proxy_request()).await {
            Ok(_) => panic!("expected error"),
            Err(err) => err,
        };
        assert!(matches!(err, ServerCoreError::NotFound(_)));
    }

    #[tokio::test]
    async fn resolve_rejects_paths_outside_allowlist() {
        let config_store = TestConfigStore::default();
        config_store.proxies.lock().unwrap().insert(
            "proxy-1".to_string(),
            ProxyConfig {
                allowed_paths: Some(vec!["/allowed".to_string()]),
                ..proxy_config(ProxyAuthMethod::HmacSigned {
                    hmac_secret_env: "PROXY_HMAC".to_string(),
                })
            },
        );
        let resolver = TestSecretResolver::default();
        resolver
            .platform_secrets
            .lock()
            .unwrap()
            .insert("PROXY_HMAC".to_string(), "secret".to_string());
        let usage_store = TestUsageStore::default();
        let service = ProxyService::new(&config_store, &resolver, &usage_store);

        let mut request = proxy_request();
        request.path = "/forbidden".to_string();

        let result = service.resolve(request).await.unwrap();
        assert_rejected(result, 403, "path_not_allowed");
    }

    #[tokio::test]
    async fn resolve_rejects_bodies_that_exceed_size_limits() {
        let config_store = TestConfigStore::default();
        config_store.proxies.lock().unwrap().insert(
            "proxy-1".to_string(),
            ProxyConfig {
                validation: Some(ProxyValidation {
                    allowed_values: HashMap::new(),
                    max_body_bytes: Some(4),
                }),
                ..proxy_config(ProxyAuthMethod::HmacSigned {
                    hmac_secret_env: "PROXY_HMAC".to_string(),
                })
            },
        );
        let resolver = TestSecretResolver::default();
        resolver
            .platform_secrets
            .lock()
            .unwrap()
            .insert("PROXY_HMAC".to_string(), "secret".to_string());
        let usage_store = TestUsageStore::default();
        let service = ProxyService::new(&config_store, &resolver, &usage_store);

        let mut request = proxy_request();
        request.body = Some(b"12345".to_vec());

        let result = service.resolve(request).await.unwrap();
        assert_rejected(result, 413, "body_too_large");
    }

    #[tokio::test]
    async fn resolve_rejects_disallowed_json_values() {
        let config_store = TestConfigStore::default();
        config_store.proxies.lock().unwrap().insert(
            "proxy-1".to_string(),
            ProxyConfig {
                validation: Some(ProxyValidation {
                    allowed_values: HashMap::from([(
                        "model".to_string(),
                        vec!["gpt-4o-mini".to_string()],
                    )]),
                    max_body_bytes: None,
                }),
                ..proxy_config(ProxyAuthMethod::HmacSigned {
                    hmac_secret_env: "PROXY_HMAC".to_string(),
                })
            },
        );
        let resolver = TestSecretResolver::default();
        resolver
            .platform_secrets
            .lock()
            .unwrap()
            .insert("PROXY_HMAC".to_string(), "secret".to_string());
        let usage_store = TestUsageStore::default();
        let service = ProxyService::new(&config_store, &resolver, &usage_store);

        let mut request = proxy_request();
        request.body = Some(br#"{"model":"not-allowed"}"#.to_vec());

        let result = service.resolve(request).await.unwrap();
        assert_rejected(result, 400, "value_not_allowed");
    }

    #[tokio::test]
    async fn resolve_rejects_plus_gated_platform_proxies_for_free_users() {
        let config_store = TestConfigStore::default();
        config_store.proxies.lock().unwrap().insert(
            "proxy-1".to_string(),
            proxy_config(ProxyAuthMethod::PlatformSecret {
                env_key: "OPENAI_KEY".to_string(),
                auth_header: "Authorization".to_string(),
                auth_prefix: "Bearer ".to_string(),
                required_tier: UserTier::Plus,
            }),
        );
        let resolver = TestSecretResolver::default();
        let usage_store = TestUsageStore::default();
        let service = ProxyService::new(&config_store, &resolver, &usage_store);

        let mut request = proxy_request();
        request.user_tier = UserTier::Free;

        let result = service.resolve(request).await.unwrap();
        assert_rejected(result, 403, "plus_required");
    }

    #[tokio::test]
    async fn resolve_rejects_rate_limited_requests() {
        let config_store = TestConfigStore::default();
        config_store.proxies.lock().unwrap().insert(
            "proxy-1".to_string(),
            ProxyConfig {
                rate_limit_per_minute: Some(5),
                ..proxy_config(ProxyAuthMethod::HmacSigned {
                    hmac_secret_env: "PROXY_HMAC".to_string(),
                })
            },
        );
        let resolver = TestSecretResolver::default();
        resolver
            .platform_secrets
            .lock()
            .unwrap()
            .insert("PROXY_HMAC".to_string(), "secret".to_string());
        let usage_store = TestUsageStore::default();
        *usage_store.rate_limit_allowed.lock().unwrap() = false;
        let service = ProxyService::new(&config_store, &resolver, &usage_store);

        let result = service.resolve(proxy_request()).await.unwrap();
        assert_rejected(result, 429, "rate_limited");
    }

    #[tokio::test]
    async fn resolve_rejects_monthly_quota_overages() {
        let config_store = TestConfigStore::default();
        config_store.proxies.lock().unwrap().insert(
            "proxy-1".to_string(),
            ProxyConfig {
                monthly_quota: Some(2),
                ..proxy_config(ProxyAuthMethod::HmacSigned {
                    hmac_secret_env: "PROXY_HMAC".to_string(),
                })
            },
        );
        let resolver = TestSecretResolver::default();
        resolver
            .platform_secrets
            .lock()
            .unwrap()
            .insert("PROXY_HMAC".to_string(), "secret".to_string());
        let usage_store = TestUsageStore::default();
        *usage_store.monthly_count.lock().unwrap() = 2;
        let service = ProxyService::new(&config_store, &resolver, &usage_store);

        let result = service.resolve(proxy_request()).await.unwrap();
        assert_rejected(result, 429, "quota_exceeded");
    }

    #[tokio::test]
    async fn resolve_builds_forward_request_for_platform_secrets() {
        let config_store = TestConfigStore::default();
        config_store.proxies.lock().unwrap().insert(
            "proxy-1".to_string(),
            ProxyConfig {
                allowed_paths: Some(vec!["/chat".to_string()]),
                streaming: true,
                ..proxy_config(ProxyAuthMethod::PlatformSecret {
                    env_key: "OPENAI_KEY".to_string(),
                    auth_header: "Authorization".to_string(),
                    auth_prefix: "Bearer ".to_string(),
                    required_tier: UserTier::Free,
                })
            },
        );
        let resolver = TestSecretResolver::default();
        resolver
            .platform_secrets
            .lock()
            .unwrap()
            .insert("OPENAI_KEY".to_string(), "secret-123".to_string());
        let usage_store = TestUsageStore::default();
        let service = ProxyService::new(&config_store, &resolver, &usage_store);

        let result = service.resolve(proxy_request()).await.unwrap();
        match result {
            ProxyResult::Forward(forward) => {
                assert_eq!(forward.url, "https://api.example.com/v1/chat/completions");
                assert_eq!(
                    forward.headers.get("Authorization").map(String::as_str),
                    Some("Bearer secret-123")
                );
                assert_eq!(
                    forward.headers.get("X-Client").map(String::as_str),
                    Some("diaryx-tests")
                );
                assert!(forward.streaming);
                assert_eq!(forward.body, Some(br#"{"model":"gpt-4o-mini"}"#.to_vec()));
            }
            ProxyResult::Rejected { .. } => panic!("expected forward"),
        }
    }

    #[tokio::test]
    async fn resolve_errors_when_platform_secret_is_missing() {
        let config_store = TestConfigStore::default();
        config_store.proxies.lock().unwrap().insert(
            "proxy-1".to_string(),
            proxy_config(ProxyAuthMethod::PlatformSecret {
                env_key: "OPENAI_KEY".to_string(),
                auth_header: "Authorization".to_string(),
                auth_prefix: "Bearer ".to_string(),
                required_tier: UserTier::Free,
            }),
        );
        let resolver = TestSecretResolver::default();
        let usage_store = TestUsageStore::default();
        let service = ProxyService::new(&config_store, &resolver, &usage_store);

        let err = match service.resolve(proxy_request()).await {
            Ok(_) => panic!("expected error"),
            Err(err) => err,
        };
        assert!(matches!(err, ServerCoreError::Internal(_)));
    }

    #[tokio::test]
    async fn resolve_builds_forward_request_for_user_secrets() {
        let config_store = TestConfigStore::default();
        config_store.proxies.lock().unwrap().insert(
            "proxy-1".to_string(),
            proxy_config(ProxyAuthMethod::UserSecret {
                secret_key: "openai".to_string(),
                auth_header: "Authorization".to_string(),
                auth_prefix: "Bearer ".to_string(),
            }),
        );
        let resolver = TestSecretResolver::default();
        resolver.user_secrets.lock().unwrap().insert(
            ("user1".to_string(), "openai".to_string()),
            "user-secret".to_string(),
        );
        let usage_store = TestUsageStore::default();
        let service = ProxyService::new(&config_store, &resolver, &usage_store);

        let result = service.resolve(proxy_request()).await.unwrap();
        match result {
            ProxyResult::Forward(forward) => {
                assert_eq!(
                    forward.headers.get("Authorization").map(String::as_str),
                    Some("Bearer user-secret")
                );
            }
            ProxyResult::Rejected { .. } => panic!("expected forward"),
        }
    }

    #[tokio::test]
    async fn resolve_errors_when_user_secret_is_missing() {
        let config_store = TestConfigStore::default();
        config_store.proxies.lock().unwrap().insert(
            "proxy-1".to_string(),
            proxy_config(ProxyAuthMethod::UserSecret {
                secret_key: "openai".to_string(),
                auth_header: "Authorization".to_string(),
                auth_prefix: "Bearer ".to_string(),
            }),
        );
        let resolver = TestSecretResolver::default();
        let usage_store = TestUsageStore::default();
        let service = ProxyService::new(&config_store, &resolver, &usage_store);

        let err = match service.resolve(proxy_request()).await {
            Ok(_) => panic!("expected error"),
            Err(err) => err,
        };
        assert!(matches!(err, ServerCoreError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn resolve_signs_hmac_requests_for_developer_proxies() {
        let config_store = TestConfigStore::default();
        config_store.proxies.lock().unwrap().insert(
            "proxy-1".to_string(),
            proxy_config(ProxyAuthMethod::HmacSigned {
                hmac_secret_env: "PROXY_HMAC".to_string(),
            }),
        );
        let resolver = TestSecretResolver::default();
        resolver
            .platform_secrets
            .lock()
            .unwrap()
            .insert("PROXY_HMAC".to_string(), "secret".to_string());
        let usage_store = TestUsageStore::default();
        let service = ProxyService::new(&config_store, &resolver, &usage_store);

        let result = service.resolve(proxy_request()).await.unwrap();
        match result {
            ProxyResult::Forward(forward) => {
                let timestamp = forward
                    .headers
                    .get("X-Diaryx-Timestamp")
                    .unwrap()
                    .parse::<u64>()
                    .unwrap();
                let user_id = forward.headers.get("X-Diaryx-User").unwrap();
                let signature = forward.headers.get("X-Diaryx-Signature").unwrap();
                assert_eq!(user_id, "user1");
                assert!(verify_proxy_signature(
                    b"secret",
                    timestamp,
                    user_id,
                    forward.body.as_deref().unwrap_or(&[]),
                    signature,
                ));
            }
            ProxyResult::Rejected { .. } => panic!("expected forward"),
        }
    }

    #[tokio::test]
    async fn record_usage_tracks_monthly_usage_for_current_period() {
        let config_store = TestConfigStore::default();
        let resolver = TestSecretResolver::default();
        let usage_store = TestUsageStore::default();
        let service = ProxyService::new(&config_store, &resolver, &usage_store);

        service.record_usage("user1", "proxy-1").await.unwrap();

        let calls = usage_store.increment_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].user_id, "user1");
        assert_eq!(calls[0].proxy_id, "proxy-1");
        assert_eq!(calls[0].period.len(), 7);
    }
}
