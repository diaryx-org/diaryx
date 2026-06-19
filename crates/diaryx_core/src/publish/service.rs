//! The publish orchestration: diff collected sources against the namespace,
//! upload the changes via a [`NamespaceProvider`], and trigger the server build.
//!
//! This is the algorithm that used to live in the Extism plugin's
//! `compute_publish_plan` + apply. It is transport-agnostic (works over the
//! port) and unit-testable with a fake provider.

use std::collections::HashMap;

use crate::publish::plan::{self, AudiencePlan, PublishPlan};
use crate::publish::provider::NamespaceProvider;
use crate::publish::source::AudienceInput;

/// Outcome of applying a publish plan.
#[derive(Debug, Clone, Default)]
pub struct PublishOutcome {
    pub uploaded: usize,
    pub bytes_uploaded: u64,
    pub deleted: usize,
    pub audiences_deleted: Vec<String>,
    pub built: bool,
}

/// Orchestrates publish against a [`NamespaceProvider`].
pub struct PublishService<'p> {
    provider: &'p dyn NamespaceProvider,
}

impl<'p> PublishService<'p> {
    pub fn new(provider: &'p dyn NamespaceProvider) -> Self {
        Self { provider }
    }

    /// Compute the diff plan for `audiences` against the namespace's current
    /// state. Uploads are sources + attachments only; server-generated keys are
    /// excluded so the publish never prunes the live rendered site.
    pub async fn compute_plan(
        &self,
        ns_id: &str,
        audiences: &[AudienceInput],
        audiences_to_delete: Vec<String>,
        audiences_migrated: bool,
    ) -> Result<PublishPlan, String> {
        // List existing objects once; index by audience → (key → content_hash).
        let mut existing_by_audience: HashMap<String, HashMap<String, Option<String>>> =
            HashMap::new();
        for obj in self.provider.list_objects(ns_id).await? {
            if let Some(aud) = obj.audience {
                existing_by_audience
                    .entry(aud)
                    .or_default()
                    .insert(obj.key, obj.content_hash);
            }
        }

        let mut audience_plans: Vec<AudiencePlan> = Vec::new();

        for audience in audiences {
            let existing = existing_by_audience
                .get(&audience.name)
                .cloned()
                .unwrap_or_default();

            // Legacy "unpublished": delete every object the audience holds.
            if !audience.publish {
                let mut deletes: Vec<String> = existing.keys().cloned().collect();
                deletes.sort();
                audience_plans.push(AudiencePlan {
                    name: audience.name.clone(),
                    gates: audience.gates.clone(),
                    uploads: Vec::new(),
                    unchanged: 0,
                    deletes,
                    publish: false,
                    stale: false,
                });
                continue;
            }

            // No entries for this audience → stale; leave its objects untouched.
            if audience.sources.is_empty() {
                audience_plans.push(AudiencePlan {
                    name: audience.name.clone(),
                    gates: audience.gates.clone(),
                    uploads: Vec::new(),
                    unchanged: 0,
                    deletes: Vec::new(),
                    publish: true,
                    stale: true,
                });
                continue;
            }

            let is_public = audience.is_public();

            // Build the planned set: markdown sources (keyed by workspace path)
            // + attachments. The server build generates HTML/assets, so those
            // are not uploaded. Track each source's ARK blade + the dest HTML
            // key the ARK should resolve to.
            let mut planned: Vec<(String, Vec<u8>, String)> =
                Vec::with_capacity(audience.sources.len() + audience.attachments.len());
            let mut key_to_ark: HashMap<String, String> = HashMap::new();
            let mut key_to_object: HashMap<String, String> = HashMap::new();
            let mut index_key: Option<String> = None;

            for src in &audience.sources {
                let source_key = format!("{}/{}", audience.name, src.source_rel_path);
                let dest_object_key = format!("{}/{}", audience.name, src.dest_path);
                if let Some(ark) = &src.file_ark {
                    key_to_ark.insert(source_key.clone(), ark.clone());
                }
                key_to_object.insert(source_key.clone(), dest_object_key);
                if src.is_index && is_public {
                    index_key = Some(source_key.clone());
                }
                planned.push((
                    source_key,
                    src.source_markdown.clone().into_bytes(),
                    "text/markdown".to_string(),
                ));
            }
            for att in &audience.attachments {
                planned.push((
                    format!("{}/{}", audience.name, att.dest_rel),
                    att.bytes.clone(),
                    att.mime_type.clone(),
                ));
            }

            // Diff against only client-managed objects (sources + attachments).
            let client_existing: HashMap<String, Option<String>> = existing
                .iter()
                .filter(|(k, _)| !plan::is_server_generated_key(k))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            let mut audience_plan = plan::diff_audience(
                audience.name.clone(),
                audience.gates.clone(),
                planned,
                &client_existing,
            );
            // Tag content-source uploads so the server registers the ARK against
            // the dest HTML key. Attachments (absent from key_to_ark) stay plain.
            for up in &mut audience_plan.uploads {
                if let Some(ark) = key_to_ark.get(&up.key) {
                    up.file_ark = Some(ark.clone());
                    up.object_key = key_to_object.get(&up.key).cloned();
                    up.source_key = Some(up.key.clone());
                    up.is_index = index_key.as_deref() == Some(up.key.as_str());
                }
            }
            audience_plans.push(audience_plan);
        }

        Ok(PublishPlan::new(
            audience_plans,
            audiences_to_delete,
            audiences_migrated,
        ))
    }

    /// Apply a plan: sync gates, upload (fatal on first error, before any
    /// delete), delete stale objects (best-effort), delete stale audiences, then
    /// trigger the server-side render.
    pub async fn apply(
        &self,
        ns_id: &str,
        plan: &PublishPlan,
        base_url: Option<&str>,
    ) -> Result<PublishOutcome, String> {
        // Phase 1: sync audience gates for publishable, non-stale audiences.
        for ap in &plan.audiences {
            if ap.publish && !ap.stale {
                self.provider
                    .sync_audience(ns_id, &ap.name, &ap.gates)
                    .await
                    .map_err(|e| format!("failed to sync audience {}: {}", ap.name, e))?;
            }
        }

        // Phase 2: upload everything first. On the first error return — having
        // run NO deletes — so a failed publish never removes live content.
        let mut outcome = PublishOutcome::default();
        for ap in &plan.audiences {
            for up in &ap.uploads {
                self.provider
                    .put_object(
                        ns_id,
                        &up.key,
                        &up.bytes,
                        &up.mime_type,
                        Some(ap.name.as_str()),
                        up.file_ark.as_deref(),
                        up.source_key.as_deref(),
                        up.object_key.as_deref(),
                        up.is_index,
                    )
                    .await
                    .map_err(|e| format!("failed to upload {}: {}", up.key, e))?;
                outcome.uploaded += 1;
                outcome.bytes_uploaded += up.bytes.len() as u64;
            }
        }

        // Phase 3: deletes (best-effort; only reached when every upload succeeded).
        for ap in &plan.audiences {
            for key in &ap.deletes {
                let _ = self.provider.delete_object(ns_id, key).await;
                outcome.deleted += 1;
            }
        }

        // Phase 3b: strict-sync audience deletion.
        for name in &plan.audiences_to_delete {
            if self.provider.delete_audience(ns_id, name).await.is_ok() {
                outcome.audiences_deleted.push(name.clone());
            }
        }

        // Phase 4: server-side render. Fatal — without it the site has no HTML.
        self.provider
            .build_namespace(ns_id, base_url)
            .await
            .map_err(|e| format!("server-side render (build) failed: {e}"))?;
        outcome.built = true;

        Ok(outcome)
    }

    /// Compute and apply in one step.
    pub async fn publish(
        &self,
        ns_id: &str,
        audiences: &[AudienceInput],
        audiences_to_delete: Vec<String>,
        audiences_migrated: bool,
        base_url: Option<&str>,
    ) -> Result<(PublishPlan, PublishOutcome), String> {
        let plan = self
            .compute_plan(ns_id, audiences, audiences_to_delete, audiences_migrated)
            .await?;
        let outcome = self.apply(ns_id, &plan, base_url).await?;
        Ok((plan, outcome))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::publish::provider::ObjectMeta;
    use crate::publish::source::{Attachment, SourceFile};
    use std::sync::Mutex;

    /// Records every provider call and serves a fixed existing-object set.
    #[derive(Default)]
    struct FakeProvider {
        existing: Vec<ObjectMeta>,
        puts: Mutex<Vec<(String, Option<String>, Option<String>, bool)>>, // key, object_key, source_key, is_index
        deletes: Mutex<Vec<String>>,
        synced: Mutex<Vec<String>>,
        built: Mutex<Vec<Option<String>>>,
    }

    #[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
    #[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
    impl NamespaceProvider for FakeProvider {
        async fn list_objects(&self, _ns_id: &str) -> Result<Vec<ObjectMeta>, String> {
            Ok(self.existing.clone())
        }
        async fn put_object(
            &self,
            _ns_id: &str,
            key: &str,
            _bytes: &[u8],
            _mime_type: &str,
            _audience: Option<&str>,
            _file_ark: Option<&str>,
            source_key: Option<&str>,
            object_key: Option<&str>,
            is_index: bool,
        ) -> Result<(), String> {
            self.puts.lock().unwrap().push((
                key.to_string(),
                object_key.map(String::from),
                source_key.map(String::from),
                is_index,
            ));
            Ok(())
        }
        async fn delete_object(&self, _ns_id: &str, key: &str) -> Result<(), String> {
            self.deletes.lock().unwrap().push(key.to_string());
            Ok(())
        }
        async fn sync_audience(
            &self,
            _ns_id: &str,
            audience: &str,
            _gates: &serde_json::Value,
        ) -> Result<(), String> {
            self.synced.lock().unwrap().push(audience.to_string());
            Ok(())
        }
        async fn list_audiences(&self, _ns_id: &str) -> Result<Vec<String>, String> {
            Ok(vec![])
        }
        async fn delete_audience(&self, _ns_id: &str, _audience: &str) -> Result<(), String> {
            Ok(())
        }
        async fn build_namespace(
            &self,
            _ns_id: &str,
            base_url: Option<&str>,
        ) -> Result<(), String> {
            self.built.lock().unwrap().push(base_url.map(String::from));
            Ok(())
        }
    }

    fn public_audience(sources: Vec<SourceFile>, attachments: Vec<Attachment>) -> AudienceInput {
        AudienceInput {
            name: "public".into(),
            gates: serde_json::json!([]),
            publish: true,
            sources,
            attachments,
        }
    }

    fn src(rel: &str, dest: &str, ark: &str, is_index: bool) -> SourceFile {
        SourceFile {
            source_markdown: format!("---\nid: {ark}\n---\nbody"),
            source_rel_path: rel.into(),
            dest_path: dest.into(),
            file_ark: Some(ark.into()),
            is_index,
        }
    }

    #[test]
    fn publish_uploads_sources_registers_dest_and_builds() {
        let provider = FakeProvider::default();
        let audiences = vec![public_audience(
            vec![
                src("Welcome.md", "index.html", "bcdfgr", true),
                src("child.md", "child.html", "bcdfgh", false),
            ],
            vec![],
        )];

        let service = PublishService::new(&provider);
        let (plan, outcome) = futures_lite::future::block_on(service.publish(
            "ns1",
            &audiences,
            vec![],
            true,
            Some("https://example.com"),
        ))
        .unwrap();

        assert_eq!(plan.totals.uploads, 2);
        assert_eq!(outcome.uploaded, 2);
        assert!(outcome.built);

        let puts = provider.puts.lock().unwrap();
        // Sources are uploaded under their workspace-relative keys...
        let root = puts
            .iter()
            .find(|(k, ..)| k == "public/Welcome.md")
            .unwrap();
        // ...registered to the dest HTML key, with the source key + index flag.
        assert_eq!(root.1.as_deref(), Some("public/index.html"));
        assert_eq!(root.2.as_deref(), Some("public/Welcome.md"));
        assert!(root.3, "root should be flagged as index");

        let child = puts.iter().find(|(k, ..)| k == "public/child.md").unwrap();
        assert_eq!(child.1.as_deref(), Some("public/child.html"));
        assert!(!child.3);

        // No HTML was uploaded — the server builds it.
        assert!(!puts.iter().any(|(k, ..)| k.ends_with(".html")));
        // Build was triggered with the base URL.
        assert_eq!(
            provider.built.lock().unwrap().as_slice(),
            &[Some("https://example.com".to_string())]
        );
        assert_eq!(
            provider.synced.lock().unwrap().as_slice(),
            &["public".to_string()]
        );
    }

    #[test]
    fn diff_excludes_server_html_and_prunes_stale_sources() {
        // Server already has rendered HTML + an old dest-keyed source no longer
        // produced. The diff must NOT delete the HTML, but SHOULD prune the
        // stale source.
        let provider = FakeProvider {
            existing: vec![
                ObjectMeta {
                    key: "public/index.html".into(),
                    audience: Some("public".into()),
                    content_hash: Some("abc".into()),
                },
                ObjectMeta {
                    key: "public/style.css".into(),
                    audience: Some("public".into()),
                    content_hash: Some("def".into()),
                },
                ObjectMeta {
                    key: "public/old.md".into(),
                    audience: Some("public".into()),
                    content_hash: Some("old".into()),
                },
            ],
            ..Default::default()
        };
        let audiences = vec![public_audience(
            vec![src("Welcome.md", "index.html", "bcdfgr", true)],
            vec![],
        )];

        let service = PublishService::new(&provider);
        let plan =
            futures_lite::future::block_on(service.compute_plan("ns1", &audiences, vec![], true))
                .unwrap();

        let deletes = &plan.audiences[0].deletes;
        assert!(
            deletes.contains(&"public/old.md".to_string()),
            "stale source should be pruned: {deletes:?}"
        );
        assert!(
            !deletes.iter().any(|k| plan::is_server_generated_key(k)),
            "server-generated keys must never be deleted: {deletes:?}"
        );
    }

    #[test]
    fn unpublished_audience_deletes_everything() {
        let provider = FakeProvider {
            existing: vec![
                ObjectMeta {
                    key: "secret/index.html".into(),
                    audience: Some("secret".into()),
                    content_hash: Some("a".into()),
                },
                ObjectMeta {
                    key: "secret/page.md".into(),
                    audience: Some("secret".into()),
                    content_hash: Some("b".into()),
                },
            ],
            ..Default::default()
        };
        let audiences = vec![AudienceInput {
            name: "secret".into(),
            gates: serde_json::json!([]),
            publish: false,
            sources: vec![],
            attachments: vec![],
        }];

        let service = PublishService::new(&provider);
        let plan =
            futures_lite::future::block_on(service.compute_plan("ns1", &audiences, vec![], true))
                .unwrap();

        // Unpublished → delete all objects (including server HTML).
        assert_eq!(plan.audiences[0].deletes.len(), 2);
        assert!(plan.audiences[0].uploads.is_empty());
    }
}
