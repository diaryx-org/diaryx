//! ARK identity registration and resolution.
//!
//! Registration happens at publish time, riding on the owner-authenticated
//! object PUT — so ownership is already enforced by the caller before this
//! service runs. The service's job is the collision check and the upsert.

use crate::domain::ArkIndexEntry;
use crate::ports::{ArkIndexStore, ServerCoreError};

/// Reserved file-blade sentinel for a workspace's front-page (index) pointer.
/// Cannot collide with a real file blade (those are exactly 6 chars from the
/// betanumeric alphabet; this contains vowels and is 5 chars).
pub const ARK_WORKSPACE_INDEX: &str = "index";

/// ARK NAAN (Name Assigning Authority Number) — the assigning-authority prefix
/// shared by every Diaryx ARK. Until Diaryx registers its own with the ARK
/// Alliance this is `99999`, the spec-reserved example NAAN, and the canonical
/// `ark:{NAAN}/{ws}/{file}` URL is accepted only as an ALIAS of the bare
/// `/ark/{ws}/{file}` form. Resolution is blade-based and never consults the
/// NAAN, so swapping in a real NAAN here is a one-line, link-preserving change.
pub const ARK_NAAN: &str = "99999";

/// A resolution inflection — what representation of a file an ARK request wants.
/// Parsed from the URL query string (the part after `?`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inflection {
    /// No query: the default rendered (HTML) object.
    Default,
    /// `?content`: the raw markdown source.
    Content,
    /// `?json`: `{ "frontmatter": {...}, "body": "..." }`.
    Json,
    /// `?info`: the frontmatter map only.
    Info,
    /// `?meta=<key>` or `?.<key>`: a single literal frontmatter field.
    Meta(String),
}

impl Inflection {
    /// Parse the raw query string. Reserved/unknown queries fall back to
    /// [`Inflection::Default`]. `meta=<key>` and a leading-dot `.<key>` both
    /// address a literal frontmatter key (so reserved names like `info` stay
    /// reachable).
    pub fn parse(query: &str) -> Self {
        match query {
            "" => Inflection::Default,
            "content" => Inflection::Content,
            "json" => Inflection::Json,
            "info" => Inflection::Info,
            q => {
                if let Some(key) = q.strip_prefix("meta=") {
                    Inflection::Meta(key.to_string())
                } else if let Some(key) = q.strip_prefix('.') {
                    Inflection::Meta(key.to_string())
                } else {
                    Inflection::Default
                }
            }
        }
    }
}

/// Build the JSON body for a `?json` / `?info` / `?meta=` inflection from the
/// stored markdown source. Parses frontmatter server-side via `diaryx_core`.
pub fn inflection_json(
    source_markdown: &str,
    inflection: &Inflection,
) -> Result<serde_json::Value, ServerCoreError> {
    let parsed = diaryx_core::frontmatter::parse_or_empty(source_markdown)
        .map_err(|e| ServerCoreError::internal(format!("frontmatter parse: {e}")))?;
    let fm = serde_json::to_value(&parsed.frontmatter)
        .map_err(|e| ServerCoreError::internal(format!("frontmatter to json: {e}")))?;
    match inflection {
        Inflection::Json => Ok(serde_json::json!({ "frontmatter": fm, "body": parsed.body })),
        Inflection::Info => Ok(fm),
        Inflection::Meta(key) => fm
            .get(key)
            .cloned()
            .ok_or_else(|| ServerCoreError::not_found("metadata key not found")),
        Inflection::Default | Inflection::Content => Err(ServerCoreError::internal(
            "inflection_json called for a non-JSON inflection",
        )),
    }
}

pub struct ArkService<'a> {
    ark_index: &'a dyn ArkIndexStore,
}

impl<'a> ArkService<'a> {
    pub fn new(ark_index: &'a dyn ArkIndexStore) -> Self {
        Self { ark_index }
    }

    /// Register the object key a file ARK resolves to within a workspace.
    ///
    /// Rejects with [`ServerCoreError::Conflict`] when the file ARK is already
    /// registered to a *different* object — a cross-device collision on a
    /// still-provisional id — so the client remints and republishes. Re-registering
    /// the same object (a republish) is idempotent.
    pub async fn register(
        &self,
        workspace_ark: &str,
        file_ark: &str,
        object_key: &str,
        audience: Option<&str>,
        source_key: Option<&str>,
    ) -> Result<(), ServerCoreError> {
        if let Some(existing_key) = self
            .ark_index
            .get_ark_owner(workspace_ark, file_ark)
            .await?
            && existing_key != object_key
        {
            return Err(ServerCoreError::conflict(
                "File ARK already registered to a different object",
            ));
        }

        self.ark_index
            .upsert_ark(workspace_ark, file_ark, object_key, audience, source_key)
            .await
    }

    /// Register the workspace front-page (index) pointer. Last-publish-wins:
    /// unlike [`register`](Self::register), this does NOT collision-check, since
    /// the index is a single workspace-level pointer any republish of the root
    /// may legitimately move.
    pub async fn register_index(
        &self,
        workspace_ark: &str,
        object_key: &str,
        audience: Option<&str>,
        source_key: Option<&str>,
    ) -> Result<(), ServerCoreError> {
        self.ark_index
            .upsert_ark(
                workspace_ark,
                ARK_WORKSPACE_INDEX,
                object_key,
                audience,
                source_key,
            )
            .await
    }

    /// Resolve a file ARK within a workspace to its current index entry.
    pub async fn resolve(
        &self,
        workspace_ark: &str,
        file_ark: &str,
    ) -> Result<ArkIndexEntry, ServerCoreError> {
        self.ark_index
            .resolve_ark(workspace_ark, file_ark)
            .await?
            .ok_or_else(|| ServerCoreError::not_found("ARK not found"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// `(workspace_ark, file_ark)` → `(object_key, audience, source_key)`.
    type ArkRows = HashMap<(String, String), (String, Option<String>, Option<String>)>;

    #[derive(Default)]
    struct TestArkIndexStore {
        rows: Mutex<ArkRows>,
    }

    crate::cfg_async_trait! {
    impl ArkIndexStore for TestArkIndexStore {
        async fn upsert_ark(
            &self,
            workspace_ark: &str,
            file_ark: &str,
            object_key: &str,
            audience: Option<&str>,
            source_key: Option<&str>,
        ) -> Result<(), ServerCoreError> {
            self.rows.lock().unwrap().insert(
                (workspace_ark.to_string(), file_ark.to_string()),
                (
                    object_key.to_string(),
                    audience.map(String::from),
                    source_key.map(String::from),
                ),
            );
            Ok(())
        }
        async fn resolve_ark(
            &self,
            workspace_ark: &str,
            file_ark: &str,
        ) -> Result<Option<ArkIndexEntry>, ServerCoreError> {
            Ok(self
                .rows
                .lock()
                .unwrap()
                .get(&(workspace_ark.to_string(), file_ark.to_string()))
                .map(|(object_key, audience, source_key)| ArkIndexEntry {
                    workspace_ark: workspace_ark.to_string(),
                    file_ark: file_ark.to_string(),
                    object_key: object_key.clone(),
                    audience: audience.clone(),
                    source_key: source_key.clone(),
                    updated_at: 1,
                }))
        }
        async fn get_ark_owner(
            &self,
            workspace_ark: &str,
            file_ark: &str,
        ) -> Result<Option<String>, ServerCoreError> {
            Ok(self
                .rows
                .lock()
                .unwrap()
                .get(&(workspace_ark.to_string(), file_ark.to_string()))
                .map(|(object_key, _, _)| object_key.clone()))
        }
        async fn list_ark_entries(
            &self,
            workspace_ark: &str,
        ) -> Result<Vec<ArkIndexEntry>, ServerCoreError> {
            Ok(self
                .rows
                .lock()
                .unwrap()
                .iter()
                .filter(|((ws, _), _)| ws == workspace_ark)
                .map(|((ws, fa), (object_key, audience, source_key))| ArkIndexEntry {
                    workspace_ark: ws.clone(),
                    file_ark: fa.clone(),
                    object_key: object_key.clone(),
                    audience: audience.clone(),
                    source_key: source_key.clone(),
                    updated_at: 1,
                })
                .collect())
        }
    }
    }

    #[tokio::test]
    async fn register_then_resolve() {
        let store = TestArkIndexStore::default();
        let service = ArkService::new(&store);

        service
            .register(
                "dxbcdfgh6",
                "bcdfgr",
                "public/note.html",
                Some("public"),
                Some("public/note.md"),
            )
            .await
            .unwrap();

        let entry = service.resolve("dxbcdfgh6", "bcdfgr").await.unwrap();
        assert_eq!(entry.object_key, "public/note.html");
        assert_eq!(entry.audience.as_deref(), Some("public"));
        assert_eq!(entry.source_key.as_deref(), Some("public/note.md"));
    }

    #[test]
    fn inflection_parsing() {
        assert_eq!(Inflection::parse(""), Inflection::Default);
        assert_eq!(Inflection::parse("content"), Inflection::Content);
        assert_eq!(Inflection::parse("json"), Inflection::Json);
        assert_eq!(Inflection::parse("info"), Inflection::Info);
        assert_eq!(
            Inflection::parse("meta=title"),
            Inflection::Meta("title".to_string())
        );
        // Leading-dot reaches a literal frontmatter key named like a reserved word.
        assert_eq!(
            Inflection::parse(".info"),
            Inflection::Meta("info".to_string())
        );
        // Reserved/unknown falls back to default.
        assert_eq!(Inflection::parse("?"), Inflection::Default);
    }

    #[test]
    fn inflection_json_shapes() {
        let src = "---\ntitle: Hello\nid: bcdfgr\n---\n\nBody text\n";

        let json = inflection_json(src, &Inflection::Json).unwrap();
        assert_eq!(json["frontmatter"]["title"], "Hello");
        assert_eq!(json["body"].as_str().unwrap().trim(), "Body text");

        let info = inflection_json(src, &Inflection::Info).unwrap();
        assert_eq!(info["title"], "Hello");
        assert_eq!(info["id"], "bcdfgr");

        let meta = inflection_json(src, &Inflection::Meta("title".to_string())).unwrap();
        assert_eq!(meta, serde_json::json!("Hello"));

        let missing = inflection_json(src, &Inflection::Meta("nope".to_string()));
        assert!(matches!(missing, Err(ServerCoreError::NotFound(_))));
    }

    #[tokio::test]
    async fn register_index_is_resolvable_and_overwrites() {
        let store = TestArkIndexStore::default();
        let service = ArkService::new(&store);

        service
            .register_index("dxbcdfgh6", "public/index.html", Some("public"), None)
            .await
            .unwrap();
        let entry = service
            .resolve("dxbcdfgh6", ARK_WORKSPACE_INDEX)
            .await
            .unwrap();
        assert_eq!(entry.object_key, "public/index.html");

        // Last-publish-wins: re-pointing the index must not 409.
        service
            .register_index("dxbcdfgh6", "family/index.html", Some("family"), None)
            .await
            .unwrap();
        let entry = service
            .resolve("dxbcdfgh6", ARK_WORKSPACE_INDEX)
            .await
            .unwrap();
        assert_eq!(entry.object_key, "family/index.html");
    }

    #[tokio::test]
    async fn resolve_missing_is_not_found() {
        let store = TestArkIndexStore::default();
        let service = ArkService::new(&store);
        let err = service.resolve("dxbcdfgh6", "bcdfgr").await.unwrap_err();
        assert!(matches!(err, ServerCoreError::NotFound(_)));
    }

    #[tokio::test]
    async fn register_is_idempotent_for_same_object() {
        let store = TestArkIndexStore::default();
        let service = ArkService::new(&store);

        service
            .register(
                "dxbcdfgh6",
                "bcdfgr",
                "public/note.html",
                Some("public"),
                None,
            )
            .await
            .unwrap();
        // Republishing the same file to the same key must succeed.
        service
            .register(
                "dxbcdfgh6",
                "bcdfgr",
                "public/note.html",
                Some("public"),
                None,
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn register_conflicts_on_different_object() {
        let store = TestArkIndexStore::default();
        let service = ArkService::new(&store);

        service
            .register(
                "dxbcdfgh6",
                "bcdfgr",
                "public/note.html",
                Some("public"),
                None,
            )
            .await
            .unwrap();

        // A second device minted the same provisional file blade for a
        // different file — must be rejected so the client remints.
        let err = service
            .register(
                "dxbcdfgh6",
                "bcdfgr",
                "public/other.html",
                Some("public"),
                None,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServerCoreError::Conflict(_)));
    }
}
