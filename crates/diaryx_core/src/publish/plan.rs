//! Pure publish-plan computation.
//!
//! A [`PublishPlan`] is the diff between what we just rendered for a namespace
//! and what the namespace already holds, so the apply step only uploads objects
//! whose content actually changed. This module performs no I/O and is fully
//! unit-testable; the orchestration lives in [`super::service`].

use std::collections::{HashMap, HashSet};

use sha2::{Digest, Sha256};

use crate::yaml;

/// Lowercase-hex SHA-256 of `bytes`.
///
/// Must match the server's `content_hash` algorithm so the diff lines up:
/// `diaryx_server::use_cases::objects::ObjectService::put` hashes the exact
/// uploaded bytes with SHA-256 and hex-encodes them lowercase.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write;
        let _ = write!(out, "{:02x}", b);
    }
    out
}

/// One object we intend the namespace to hold after publish, whose content is
/// new or changed and therefore needs uploading.
#[derive(Debug, Clone)]
pub struct PendingUpload {
    /// Object key, e.g. `"family/index.html"` or `"public/_attachments/a.jpg"`.
    pub key: String,
    /// The exact bytes to upload (already prepared, e.g. HTML bridge injected).
    pub bytes: Vec<u8>,
    pub mime_type: String,
    /// SHA-256 of `bytes` (computed once during the diff).
    pub hash: String,
    /// Source file's ARK blade, for content pages. Populated by the caller
    /// after the diff (keyed by object key); `None` for assets/attachments.
    pub file_ark: Option<String>,
    /// Object key of the markdown source sibling for this content page, if any.
    pub source_key: Option<String>,
    /// Key the ARK should resolve to (the server-rendered HTML), when it differs
    /// from this upload's `key` — set when uploading a markdown source for
    /// server-side rendering. `None` → the ARK resolves to `key` (legacy push).
    pub object_key: Option<String>,
    /// `true` when this upload is the workspace front-page (index) rendition.
    pub is_index: bool,
}

/// The diff for a single audience.
#[derive(Debug, Clone)]
pub struct AudiencePlan {
    pub name: String,
    /// Gate stack (a sequence of `{ kind: ... }` maps) for the server's
    /// `sync_audience` endpoint. Serialized to JSON at the transport boundary.
    pub gates: yaml::Value,
    /// New/changed objects to upload.
    pub uploads: Vec<PendingUpload>,
    /// Count of planned objects already present with identical content.
    pub unchanged: usize,
    /// Existing object keys for this audience no longer in the planned set.
    pub deletes: Vec<String>,
    /// Whether this audience publishes content. `false` = legacy `Unpublished`:
    /// all its objects are deleted and nothing is rendered.
    pub publish: bool,
    /// `true` when a publishable audience rendered zero entries. Its objects are
    /// left untouched (matching prior behavior); it is reported as stale so the
    /// caller can prune it from legacy config / strict-sync.
    pub stale: bool,
}

/// Aggregate counts across a whole plan, for receipts and previews.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlanTotals {
    pub uploads: usize,
    pub unchanged: usize,
    pub deletes: usize,
    pub bytes: u64,
}

/// The full set of changes a publish (or preview) would make.
#[derive(Debug, Clone)]
pub struct PublishPlan {
    pub audiences: Vec<AudiencePlan>,
    /// Server-side audiences to delete entirely (strict file-as-truth sync).
    pub audiences_to_delete: Vec<String>,
    /// Whether the workspace has explicitly migrated to file-as-truth audiences.
    pub audiences_migrated: bool,
    pub totals: PlanTotals,
}

impl PublishPlan {
    /// Build a plan, computing aggregate totals from the audience diffs.
    pub fn new(
        audiences: Vec<AudiencePlan>,
        audiences_to_delete: Vec<String>,
        audiences_migrated: bool,
    ) -> Self {
        let mut totals = PlanTotals::default();
        for a in &audiences {
            totals.uploads += a.uploads.len();
            totals.unchanged += a.unchanged;
            totals.deletes += a.deletes.len();
            totals.bytes += a.uploads.iter().map(|u| u.bytes.len() as u64).sum::<u64>();
        }
        Self {
            audiences,
            audiences_to_delete,
            audiences_migrated,
            totals,
        }
    }

    /// Summary of the plan WITHOUT object bytes — safe to return to the UI for
    /// previews and receipts. A [`yaml::Value`] so it serializes to JSON via
    /// `fig` on every platform (no `serde_json` in the WASM binary).
    pub fn to_summary_json(&self) -> yaml::Value {
        let str_seq = |items: &[String]| {
            yaml::Value::Sequence(items.iter().cloned().map(yaml::Value::String).collect())
        };
        let audiences: Vec<yaml::Value> = self
            .audiences
            .iter()
            .map(|a| {
                let uploads: Vec<yaml::Value> = a
                    .uploads
                    .iter()
                    .map(|u| {
                        map([
                            ("key", yaml::Value::String(u.key.clone())),
                            ("size", yaml::Value::Int(u.bytes.len() as i64)),
                        ])
                    })
                    .collect();
                let upload_bytes: i64 = a.uploads.iter().map(|u| u.bytes.len() as i64).sum();
                map([
                    ("name", yaml::Value::String(a.name.clone())),
                    ("publish", yaml::Value::Bool(a.publish)),
                    ("stale", yaml::Value::Bool(a.stale)),
                    ("upload_count", yaml::Value::Int(a.uploads.len() as i64)),
                    ("upload_bytes", yaml::Value::Int(upload_bytes)),
                    ("uploads", yaml::Value::Sequence(uploads)),
                    ("unchanged", yaml::Value::Int(a.unchanged as i64)),
                    ("delete_count", yaml::Value::Int(a.deletes.len() as i64)),
                    ("deletes", str_seq(&a.deletes)),
                ])
            })
            .collect();

        map([
            ("audiences", yaml::Value::Sequence(audiences)),
            ("audiences_to_delete", str_seq(&self.audiences_to_delete)),
            (
                "audiences_migrated",
                yaml::Value::Bool(self.audiences_migrated),
            ),
            (
                "totals",
                map([
                    ("uploads", yaml::Value::Int(self.totals.uploads as i64)),
                    ("unchanged", yaml::Value::Int(self.totals.unchanged as i64)),
                    ("deletes", yaml::Value::Int(self.totals.deletes as i64)),
                    ("bytes", yaml::Value::Int(self.totals.bytes as i64)),
                ]),
            ),
        ])
    }
}

/// Build a [`yaml::Value::Mapping`] from string-keyed pairs (preserving order).
fn map<const N: usize>(pairs: [(&str, yaml::Value); N]) -> yaml::Value {
    let mut m = yaml::Mapping::new();
    for (k, v) in pairs {
        m.insert(k.to_string(), v);
    }
    yaml::Value::Mapping(m)
}

/// Whether an object key names a *server-generated* artifact (rendered HTML or
/// a static/supplementary asset the server build produces), as opposed to a
/// client-managed object (a markdown source `.md` or an attachment).
///
/// Under server-side rendering the client uploads only sources + attachments
/// and the server owns all rendered output. The publish diff must therefore
/// ignore server-generated keys entirely — never matching them and, crucially,
/// never deleting them — or every publish would prune the live rendered site.
pub fn is_server_generated_key(key: &str) -> bool {
    let base = key.rsplit('/').next().unwrap_or(key);
    key.ends_with(".html")
        || base == "style.css"
        || base == "sitemap.xml"
        || base == "robots.txt"
        || base == "feed.xml"
        || base == "rss.xml"
        || base.starts_with("favicon.")
}

/// Diff a single publishable audience's freshly-rendered objects against the
/// objects the namespace already holds for that audience.
///
/// - planned key whose existing hash equals the fresh SHA-256 → **unchanged**
///   (skipped, but kept in the planned set so it is not deleted);
/// - planned key whose existing hash differs or is absent/`None` → **upload**;
/// - existing key not in the planned set → **delete**.
///
/// `existing` maps object key → its server-reported `content_hash` (`None` when
/// the server never recorded one, which we conservatively treat as changed).
pub fn diff_audience(
    name: String,
    gates: yaml::Value,
    planned: Vec<(String, Vec<u8>, String)>,
    existing: &HashMap<String, Option<String>>,
) -> AudiencePlan {
    let planned_keys: HashSet<String> = planned.iter().map(|(k, _, _)| k.clone()).collect();

    let mut uploads = Vec::new();
    let mut unchanged = 0usize;
    for (key, bytes, mime_type) in planned {
        let hash = sha256_hex(&bytes);
        let matches = existing
            .get(&key)
            .and_then(|h| h.as_deref())
            .map(|h| h == hash)
            .unwrap_or(false);
        if matches {
            unchanged += 1;
        } else {
            uploads.push(PendingUpload {
                key,
                bytes,
                mime_type,
                hash,
                file_ark: None,
                source_key: None,
                object_key: None,
                is_index: false,
            });
        }
    }

    let mut deletes: Vec<String> = existing
        .keys()
        .filter(|k| !planned_keys.contains(*k))
        .cloned()
        .collect();
    deletes.sort();

    AudiencePlan {
        name,
        gates,
        uploads,
        unchanged,
        deletes,
        publish: true,
        stale: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn existing(pairs: &[(&str, Option<&str>)]) -> HashMap<String, Option<String>> {
        pairs
            .iter()
            .map(|(k, h)| (k.to_string(), h.map(|s| s.to_string())))
            .collect()
    }

    #[test]
    fn server_generated_keys_are_classified() {
        // Server-generated → excluded from the diff (never deleted by the client).
        assert!(is_server_generated_key("public/index.html"));
        assert!(is_server_generated_key("public/notes/post.html"));
        assert!(is_server_generated_key("public/style.css"));
        assert!(is_server_generated_key("public/sitemap.xml"));
        assert!(is_server_generated_key("public/robots.txt"));
        assert!(is_server_generated_key("public/feed.xml"));
        assert!(is_server_generated_key("public/rss.xml"));
        assert!(is_server_generated_key("public/favicon.svg"));
        assert!(is_server_generated_key("public/favicon.ico"));

        // Client-managed → kept in the diff (sources + attachments).
        assert!(!is_server_generated_key("public/Welcome.md"));
        assert!(!is_server_generated_key("public/notes/post.md"));
        assert!(!is_server_generated_key("public/_attachments/image.png"));
        assert!(!is_server_generated_key("public/_attachments/diagram.svg"));
    }

    #[test]
    fn sha256_matches_known_vector() {
        // Standard SHA-256("hello") test vector, lowercase hex.
        assert_eq!(
            sha256_hex(b"hello"),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn new_file_is_uploaded() {
        let plan = diff_audience(
            "pub".into(),
            yaml::Value::Sequence(vec![]),
            vec![("pub/a.html".into(), b"hi".to_vec(), "text/html".into())],
            &existing(&[]),
        );
        assert_eq!(plan.uploads.len(), 1);
        assert_eq!(plan.unchanged, 0);
        assert!(plan.deletes.is_empty());
    }

    #[test]
    fn unchanged_file_is_skipped() {
        let hash = sha256_hex(b"hi");
        let plan = diff_audience(
            "pub".into(),
            yaml::Value::Sequence(vec![]),
            vec![("pub/a.html".into(), b"hi".to_vec(), "text/html".into())],
            &existing(&[("pub/a.html", Some(hash.as_str()))]),
        );
        assert!(plan.uploads.is_empty());
        assert_eq!(plan.unchanged, 1);
        assert!(plan.deletes.is_empty());
    }

    #[test]
    fn changed_file_is_uploaded() {
        let plan = diff_audience(
            "pub".into(),
            yaml::Value::Sequence(vec![]),
            vec![("pub/a.html".into(), b"new".to_vec(), "text/html".into())],
            &existing(&[("pub/a.html", Some(&sha256_hex(b"old")))]),
        );
        assert_eq!(plan.uploads.len(), 1);
        assert_eq!(plan.unchanged, 0);
    }

    #[test]
    fn missing_server_hash_forces_upload() {
        let plan = diff_audience(
            "pub".into(),
            yaml::Value::Sequence(vec![]),
            vec![("pub/a.html".into(), b"hi".to_vec(), "text/html".into())],
            &existing(&[("pub/a.html", None)]),
        );
        assert_eq!(plan.uploads.len(), 1);
        assert_eq!(plan.unchanged, 0);
    }

    #[test]
    fn stale_existing_object_is_deleted() {
        let plan = diff_audience(
            "pub".into(),
            yaml::Value::Sequence(vec![]),
            vec![("pub/a.html".into(), b"hi".to_vec(), "text/html".into())],
            &existing(&[
                ("pub/a.html", Some(&sha256_hex(b"hi"))),
                ("pub/gone.html", Some(&sha256_hex(b"x"))),
            ]),
        );
        assert_eq!(plan.unchanged, 1);
        assert_eq!(plan.deletes, vec!["pub/gone.html".to_string()]);
    }

    #[test]
    fn totals_aggregate_across_audiences() {
        let a = diff_audience(
            "a".into(),
            yaml::Value::Sequence(vec![]),
            vec![("a/x".into(), b"12345".to_vec(), "text/plain".into())],
            &existing(&[("a/old", Some("deadbeef"))]),
        );
        let b = diff_audience(
            "b".into(),
            yaml::Value::Sequence(vec![]),
            vec![("b/y".into(), b"hi".to_vec(), "text/plain".into())],
            &existing(&[("b/y", Some(&sha256_hex(b"hi")))]),
        );
        let plan = PublishPlan::new(vec![a, b], vec!["dead".into()], true);
        assert_eq!(plan.totals.uploads, 1);
        assert_eq!(plan.totals.unchanged, 1);
        assert_eq!(plan.totals.deletes, 1);
        assert_eq!(plan.totals.bytes, 5);
        assert_eq!(plan.audiences_to_delete, vec!["dead".to_string()]);
    }
}
