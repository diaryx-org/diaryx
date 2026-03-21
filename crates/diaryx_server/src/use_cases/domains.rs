use crate::domain::CustomDomainInfo;
use crate::ports::{DomainMappingCache, NamespaceStore, ServerCoreError};
use serde::{Deserialize, Serialize};

pub const RESERVED_SUBDOMAINS: &[&str] = &[
    "www", "api", "app", "mail", "smtp", "ftp", "ns", "admin", "sync", "site", "sites",
];
pub const DIARYX_SUBDOMAIN_SUFFIX: &str = ".diaryx.org";
pub const SUBDOMAIN_AUDIENCE_NAME: &str = "*";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimedSubdomain {
    pub subdomain: String,
    pub domain: String,
    pub namespace_id: String,
    pub default_audience: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleasedSubdomain {
    pub subdomain: String,
    pub domain: String,
    pub namespace_id: String,
}

pub struct DomainService<'a> {
    namespace_store: &'a dyn NamespaceStore,
    domain_mapping_cache: &'a dyn DomainMappingCache,
}

impl<'a> DomainService<'a> {
    pub fn new(
        namespace_store: &'a dyn NamespaceStore,
        domain_mapping_cache: &'a dyn DomainMappingCache,
    ) -> Self {
        Self {
            namespace_store,
            domain_mapping_cache,
        }
    }

    pub async fn register_domain(
        &self,
        namespace_id: &str,
        domain: &str,
        audience_name: &str,
    ) -> Result<CustomDomainInfo, ServerCoreError> {
        if self
            .namespace_store
            .get_audience(namespace_id, audience_name)
            .await?
            .is_none()
        {
            return Err(ServerCoreError::invalid_input(format!(
                "audience '{}' does not exist",
                audience_name
            )));
        }

        self.namespace_store
            .upsert_custom_domain(domain, namespace_id, audience_name)
            .await?;

        let domain_info = self
            .namespace_store
            .get_custom_domain(domain)
            .await?
            .ok_or_else(|| {
                ServerCoreError::internal(format!(
                    "Domain '{}' was missing after registration",
                    domain
                ))
            })?;

        self.domain_mapping_cache
            .put_domain(
                &domain_info.domain,
                &domain_info.namespace_id,
                &domain_info.audience_name,
            )
            .await?;

        Ok(domain_info)
    }

    pub async fn remove_domain(
        &self,
        namespace_id: &str,
        domain: &str,
    ) -> Result<(), ServerCoreError> {
        let existing = self
            .namespace_store
            .get_custom_domain(domain)
            .await?
            .ok_or_else(|| ServerCoreError::not_found(format!("Domain '{}' not found", domain)))?;

        if existing.namespace_id != namespace_id {
            return Err(ServerCoreError::not_found(format!(
                "Domain '{}' not found for namespace '{}'",
                domain, namespace_id
            )));
        }

        let deleted = self.namespace_store.delete_custom_domain(domain).await?;
        if !deleted {
            return Err(ServerCoreError::not_found(format!(
                "Domain '{}' not found",
                domain
            )));
        }

        self.domain_mapping_cache.delete_domain(domain).await?;
        Ok(())
    }

    pub async fn claim_subdomain(
        &self,
        namespace_id: &str,
        requested_subdomain: &str,
        default_audience: Option<&str>,
    ) -> Result<ClaimedSubdomain, ServerCoreError> {
        let subdomain = validate_subdomain_label(requested_subdomain)?;
        let domain = format!("{}{}", subdomain, DIARYX_SUBDOMAIN_SUFFIX);

        if let Some(existing) = self.namespace_store.get_custom_domain(&domain).await?
            && existing.namespace_id != namespace_id
        {
            return Err(ServerCoreError::conflict(
                "This subdomain is already taken.",
            ));
        }

        self.namespace_store
            .upsert_custom_domain(&domain, namespace_id, SUBDOMAIN_AUDIENCE_NAME)
            .await?;
        self.domain_mapping_cache
            .put_subdomain(&subdomain, namespace_id, default_audience)
            .await?;

        Ok(ClaimedSubdomain {
            subdomain,
            domain,
            namespace_id: namespace_id.to_string(),
            default_audience: default_audience.map(str::to_string),
        })
    }

    pub async fn release_subdomain(
        &self,
        namespace_id: &str,
    ) -> Result<ReleasedSubdomain, ServerCoreError> {
        let domain_info = self
            .namespace_store
            .list_custom_domains(namespace_id)
            .await?
            .into_iter()
            .find(|domain| {
                domain.audience_name == SUBDOMAIN_AUDIENCE_NAME
                    && domain.domain.ends_with(DIARYX_SUBDOMAIN_SUFFIX)
            })
            .ok_or_else(|| {
                ServerCoreError::not_found("No Diaryx subdomain registered for this namespace.")
            })?;

        let subdomain = domain_info
            .domain
            .trim_end_matches(DIARYX_SUBDOMAIN_SUFFIX)
            .to_string();

        let deleted = self
            .namespace_store
            .delete_custom_domain(&domain_info.domain)
            .await?;
        if !deleted {
            return Err(ServerCoreError::not_found(format!(
                "Domain '{}' not found",
                domain_info.domain
            )));
        }

        self.domain_mapping_cache
            .delete_subdomain(&subdomain)
            .await?;

        Ok(ReleasedSubdomain {
            subdomain,
            domain: domain_info.domain,
            namespace_id: namespace_id.to_string(),
        })
    }
}

pub fn normalize_subdomain_label(input: &str) -> String {
    input.trim().to_lowercase()
}

pub fn validate_subdomain_label(input: &str) -> Result<String, ServerCoreError> {
    let subdomain = normalize_subdomain_label(input);

    if subdomain.len() < 3
        || subdomain.len() > 63
        || !subdomain
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
        || subdomain.starts_with('-')
        || subdomain.ends_with('-')
    {
        return Err(ServerCoreError::invalid_input(
            "Invalid subdomain. Use 3-63 alphanumeric characters and hyphens.".to_string(),
        ));
    }

    if RESERVED_SUBDOMAINS.contains(&subdomain.as_str()) {
        return Err(ServerCoreError::conflict("This subdomain is reserved."));
    }

    Ok(subdomain)
}

#[cfg(test)]
mod tests {
    use super::{
        ClaimedSubdomain, DIARYX_SUBDOMAIN_SUFFIX, DomainService, SUBDOMAIN_AUDIENCE_NAME,
        validate_subdomain_label,
    };
    use crate::domain::{AudienceInfo, CustomDomainInfo, NamespaceInfo};
    use crate::ports::{DomainMappingCache, NamespaceStore, ServerCoreError};
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct TestNamespaceStore {
        audiences: Mutex<HashMap<(String, String), AudienceInfo>>,
        domains: Mutex<HashMap<String, CustomDomainInfo>>,
    }

    crate::cfg_async_trait! {
    impl NamespaceStore for TestNamespaceStore {
        async fn get_namespace(
            &self,
            _namespace_id: &str,
        ) -> Result<Option<NamespaceInfo>, ServerCoreError> {
            Ok(None)
        }

        async fn list_namespaces(
            &self,
            _owner_user_id: &str,
            _limit: u32,
            _offset: u32,
        ) -> Result<Vec<NamespaceInfo>, ServerCoreError> {
            Ok(vec![])
        }

        async fn get_audience(
            &self,
            namespace_id: &str,
            audience_name: &str,
        ) -> Result<Option<AudienceInfo>, ServerCoreError> {
            Ok(self
                .audiences
                .lock()
                .expect("audiences lock")
                .get(&(namespace_id.to_string(), audience_name.to_string()))
                .cloned())
        }

        async fn get_custom_domain(
            &self,
            domain: &str,
        ) -> Result<Option<CustomDomainInfo>, ServerCoreError> {
            Ok(self
                .domains
                .lock()
                .expect("domains lock")
                .get(domain)
                .cloned())
        }

        async fn list_custom_domains(
            &self,
            namespace_id: &str,
        ) -> Result<Vec<CustomDomainInfo>, ServerCoreError> {
            Ok(self
                .domains
                .lock()
                .expect("domains lock")
                .values()
                .filter(|domain| domain.namespace_id == namespace_id)
                .cloned()
                .collect())
        }

        async fn upsert_custom_domain(
            &self,
            domain: &str,
            namespace_id: &str,
            audience_name: &str,
        ) -> Result<(), ServerCoreError> {
            let mut domains = self.domains.lock().expect("domains lock");
            let created_at = domains
                .get(domain)
                .map(|domain| domain.created_at)
                .unwrap_or(1);
            domains.insert(
                domain.to_string(),
                CustomDomainInfo {
                    domain: domain.to_string(),
                    namespace_id: namespace_id.to_string(),
                    audience_name: audience_name.to_string(),
                    created_at,
                    verified: false,
                },
            );
            Ok(())
        }

        async fn delete_custom_domain(&self, domain: &str) -> Result<bool, ServerCoreError> {
            Ok(self
                .domains
                .lock()
                .expect("domains lock")
                .remove(domain)
                .is_some())
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
    }
    }

    #[derive(Default)]
    struct TestDomainMappingCache {
        puts: Mutex<Vec<String>>,
        deletes: Mutex<Vec<String>>,
    }

    crate::cfg_async_trait! {
    impl DomainMappingCache for TestDomainMappingCache {
        async fn put_domain(
            &self,
            hostname: &str,
            namespace_id: &str,
            audience_name: &str,
        ) -> Result<(), ServerCoreError> {
            self.puts.lock().expect("puts lock").push(format!(
                "domain:{}:{}:{}",
                hostname, namespace_id, audience_name
            ));
            Ok(())
        }

        async fn delete_domain(&self, hostname: &str) -> Result<(), ServerCoreError> {
            self.deletes
                .lock()
                .expect("deletes lock")
                .push(format!("domain:{}", hostname));
            Ok(())
        }

        async fn put_subdomain(
            &self,
            subdomain: &str,
            namespace_id: &str,
            default_audience: Option<&str>,
        ) -> Result<(), ServerCoreError> {
            self.puts.lock().expect("puts lock").push(format!(
                "subdomain:{}:{}:{}",
                subdomain,
                namespace_id,
                default_audience.unwrap_or("")
            ));
            Ok(())
        }

        async fn delete_subdomain(&self, subdomain: &str) -> Result<(), ServerCoreError> {
            self.deletes
                .lock()
                .expect("deletes lock")
                .push(format!("subdomain:{}", subdomain));
            Ok(())
        }
    }
    }

    #[test]
    fn accepts_normalized_subdomain() {
        assert_eq!(validate_subdomain_label("Notes-App").unwrap(), "notes-app");
    }

    #[test]
    fn rejects_reserved_subdomain() {
        assert!(validate_subdomain_label("app").is_err());
    }

    #[test]
    fn rejects_invalid_characters() {
        assert!(validate_subdomain_label("bad_name").is_err());
    }

    #[tokio::test]
    async fn register_domain_requires_existing_audience_and_updates_cache() {
        let store = TestNamespaceStore::default();
        store.audiences.lock().expect("audiences lock").insert(
            ("ns_123".to_string(), "public".to_string()),
            AudienceInfo {
                namespace_id: "ns_123".to_string(),
                audience_name: "public".to_string(),
                access: "public".to_string(),
            },
        );
        let cache = TestDomainMappingCache::default();
        let service = DomainService::new(&store, &cache);

        let domain = service
            .register_domain("ns_123", "blog.example.com", "public")
            .await
            .expect("domain should register");

        assert_eq!(domain.domain, "blog.example.com");
        assert_eq!(
            *cache.puts.lock().expect("puts lock"),
            vec!["domain:blog.example.com:ns_123:public".to_string()]
        );
    }

    #[tokio::test]
    async fn claim_and_release_subdomain_round_trips_through_shared_service() {
        let store = TestNamespaceStore::default();
        let cache = TestDomainMappingCache::default();
        let service = DomainService::new(&store, &cache);

        let claimed = service
            .claim_subdomain("ns_123", "Notes-App", Some("public"))
            .await
            .expect("subdomain should be claimed");
        assert_eq!(
            claimed,
            ClaimedSubdomain {
                subdomain: "notes-app".to_string(),
                domain: format!("notes-app{}", DIARYX_SUBDOMAIN_SUFFIX),
                namespace_id: "ns_123".to_string(),
                default_audience: Some("public".to_string()),
            }
        );

        let stored = store
            .domains
            .lock()
            .expect("domains lock")
            .get(&claimed.domain)
            .cloned()
            .expect("stored domain");
        assert_eq!(stored.audience_name, SUBDOMAIN_AUDIENCE_NAME);

        let released = service
            .release_subdomain("ns_123")
            .await
            .expect("subdomain should release");
        assert_eq!(released.subdomain, "notes-app");
        assert_eq!(
            *cache.deletes.lock().expect("deletes lock"),
            vec!["subdomain:notes-app".to_string()]
        );
    }
}
