//! Unit tests for [`diaryx_server::use_cases::objects::ObjectService`] driven
//! against the in-memory stores from [`diaryx_server::testing`].
//!
//! These tests exercise the pure platform-agnostic logic: owner checks,
//! content-hash dedup, metadata round-trip, audience validation. Adapter-
//! specific concerns (SQL, R2, HTTP routing, URL decoding) belong in the
//! adapter crates.

use diaryx_server::ports::{BlobStore, NamespaceStore, ObjectMetaStore, ServerCoreError};
use diaryx_server::testing::{InMemoryBlobStore, InMemoryNamespaceStore, InMemoryObjectMetaStore};
use diaryx_server::use_cases::objects::ObjectService;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

struct Fixture {
    ns: InMemoryNamespaceStore,
    meta: InMemoryObjectMetaStore,
    blob: InMemoryBlobStore,
}

impl Fixture {
    fn new() -> Self {
        Self {
            ns: InMemoryNamespaceStore::new(),
            meta: InMemoryObjectMetaStore::new(),
            blob: InMemoryBlobStore::with_prefix("test"),
        }
    }

    async fn with_namespace(self, namespace_id: &str, owner_user_id: &str) -> Self {
        self.ns
            .create_namespace(namespace_id, owner_user_id, None)
            .await
            .expect("namespace should be created");
        self
    }

    fn service(&self) -> ObjectService<'_> {
        ObjectService::new(&self.ns, &self.meta, &self.blob)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn put_stores_blob_and_metadata() {
    let f = Fixture::new().with_namespace("ns-1", "user-1").await;
    let svc = f.service();

    let result = svc
        .put(
            "ns-1",
            "hello.md",
            "text/markdown",
            b"# Hello",
            None,
            "user-1",
        )
        .await
        .expect("put should succeed");

    assert_eq!(result.key, "hello.md");
    assert_eq!(result.size_bytes, 7);

    // Metadata landed.
    let meta = f
        .meta
        .get_object_meta("ns-1", "hello.md")
        .await
        .expect("meta lookup should succeed")
        .expect("object should have metadata");
    assert_eq!(meta.size_bytes, 7);
    assert_eq!(meta.mime_type, "text/markdown");
    assert!(meta.content_hash.is_some(), "content hash should be set");

    // Blob landed.
    let blob_key = meta.blob_key.as_deref().expect("blob_key should be set");
    assert!(
        f.blob.exists(blob_key).await.unwrap(),
        "blob should exist at {blob_key}"
    );
    let bytes = f.blob.get(blob_key).await.unwrap().unwrap();
    assert_eq!(bytes, b"# Hello");
}

#[tokio::test]
async fn put_denies_when_caller_is_not_owner() {
    let f = Fixture::new().with_namespace("ns-1", "owner").await;
    let svc = f.service();

    let err = svc
        .put(
            "ns-1",
            "secret.md",
            "text/markdown",
            b"nope",
            None,
            "intruder",
        )
        .await
        .expect_err("put should be denied for non-owner");

    assert!(
        matches!(err, ServerCoreError::PermissionDenied(_)),
        "expected PermissionDenied, got {err:?}"
    );

    // Nothing leaked into the stores.
    assert!(
        f.meta
            .get_object_meta("ns-1", "secret.md")
            .await
            .unwrap()
            .is_none(),
        "no metadata should have been written"
    );
    assert!(
        f.blob.raw_blobs().is_empty(),
        "no blobs should have been written, got: {:?}",
        f.blob.raw_blobs()
    );
}

#[tokio::test]
async fn put_404s_on_missing_namespace() {
    let f = Fixture::new();
    let svc = f.service();

    let err = svc
        .put("ns-nope", "x.md", "text/plain", b"x", None, "user-1")
        .await
        .expect_err("missing namespace should error");

    assert!(
        matches!(err, ServerCoreError::NotFound(_)),
        "expected NotFound, got {err:?}"
    );
}

#[tokio::test]
async fn put_rejects_unknown_audience() {
    let f = Fixture::new().with_namespace("ns-1", "user-1").await;
    let svc = f.service();

    let err = svc
        .put(
            "ns-1",
            "post.md",
            "text/markdown",
            b"hi",
            Some("nonexistent-audience"),
            "user-1",
        )
        .await
        .expect_err("put with undeclared audience should fail");

    assert!(
        matches!(err, ServerCoreError::InvalidInput(_)),
        "expected InvalidInput, got {err:?}"
    );
}

#[tokio::test]
async fn put_deduplicates_identical_content() {
    let f = Fixture::new().with_namespace("ns-1", "user-1").await;
    let svc = f.service();

    svc.put(
        "ns-1",
        "a.md",
        "text/markdown",
        b"same bytes",
        None,
        "user-1",
    )
    .await
    .expect("first put");
    svc.put(
        "ns-1",
        "b.md",
        "text/markdown",
        b"same bytes",
        None,
        "user-1",
    )
    .await
    .expect("second put");

    // Two metadata rows, one blob.
    assert!(
        f.meta
            .get_object_meta("ns-1", "a.md")
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        f.meta
            .get_object_meta("ns-1", "b.md")
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(
        f.blob.raw_blobs().len(),
        1,
        "content-hash dedup should yield a single blob, got: {:?}",
        f.blob.raw_blobs()
    );
}

/// Keys from the shared URL encoding corpus round-trip through metadata —
/// at the logic layer these are just opaque byte strings. Transport-level
/// encoding is the adapter's job and is covered by the HTTP contract suite.
#[tokio::test]
async fn put_accepts_every_key_in_url_corpus() {
    use diaryx_server::contract::URL_KEY_CORPUS;

    let f = Fixture::new().with_namespace("ns-1", "user-1").await;
    let svc = f.service();

    for (i, key) in URL_KEY_CORPUS.iter().enumerate() {
        let bytes = format!("body-{i}").into_bytes();
        svc.put("ns-1", key, "text/plain", &bytes, None, "user-1")
            .await
            .unwrap_or_else(|e| panic!("put failed for key {key:?}: {e:?}"));

        let meta = f
            .meta
            .get_object_meta("ns-1", key)
            .await
            .unwrap()
            .unwrap_or_else(|| panic!("metadata missing for key {key:?}"));
        assert_eq!(
            meta.key, *key,
            "stored key should match input key verbatim at this layer"
        );
    }
}
