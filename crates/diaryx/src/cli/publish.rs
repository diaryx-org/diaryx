//! CLI handlers for publishing a workspace to a server namespace, plus the
//! `NamespaceProvider` HTTP adapter the publish pipeline drives.
//!
//! Publishing is the new `diaryx_core::publish` path (no Extism plugin): the
//! workspace's file-declared audiences are collected, diffed against the
//! namespace's current objects, uploaded, and the namespace is rebuilt
//! server-side (ARK Layer 3). The workspace ARK *is* the server `namespace_id`;
//! when none is bound (or `--new-namespace` is passed) the CLI mints a fresh
//! `dx`-blade locally and registers it, so published content resolves at
//! `https://diaryx.org/ark/{namespace_id}/...`.

use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Duration;

use diaryx_core::auth::AuthenticatedClient;
use diaryx_core::fs::SyncToAsyncFs;
use diaryx_core::publish::{NamespaceProvider, ObjectMeta, PublishService};
use diaryx_core::workspace::Workspace;
use diaryx_core::{fig, namespace, yaml};
use diaryx_native::RealFileSystem;

use super::auth_client::FsAuthenticatedClient;
use super::block_on;
use super::export::resolve_workspace_for_export;

// ---------------------------------------------------------------------------
// HTTP NamespaceProvider — the server-talking seam for the publish pipeline.
// ---------------------------------------------------------------------------

/// A [`NamespaceProvider`] backed by the CLI's authenticated HTTP session.
///
/// Built from the shared [`FsAuthenticatedClient`] (server URL + bearer token);
/// owns its own blocking `ureq` agent. The async-trait methods call `ureq`
/// synchronously — fine under the CLI's single-threaded `block_on` executor.
pub struct HttpNamespaceProvider {
    server_url: String,
    token: Option<String>,
    agent: ureq::Agent,
}

enum NoBody {
    Get,
    Delete,
}

enum WithBody {
    Put,
    Post,
}

impl HttpNamespaceProvider {
    pub fn from_client(client: &FsAuthenticatedClient) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(60)))
            .http_status_as_error(false)
            .build()
            .new_agent();
        Self {
            server_url: client.server_url().trim_end_matches('/').to_string(),
            token: client.export_bearer_token(),
            agent,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.server_url, path)
    }

    fn enc(value: &str) -> String {
        urlencoding::encode(value).into_owned()
    }

    /// Percent-encode a `/`-delimited object key segment-by-segment, leaving
    /// the separators intact (the server route is `{*key}`).
    fn enc_key(key: &str) -> String {
        key.split('/').map(Self::enc).collect::<Vec<_>>().join("/")
    }

    fn bearer(&self) -> Option<String> {
        self.token.as_ref().map(|t| format!("Bearer {t}"))
    }

    /// GET/DELETE — no request body. Returns `(status, body)`.
    fn no_body(&self, method: NoBody, url: &str) -> Result<(u16, String), String> {
        let mut req = match method {
            NoBody::Get => self.agent.get(url),
            NoBody::Delete => self.agent.delete(url),
        };
        if let Some(h) = self.bearer() {
            req = req.header("Authorization", &h);
        }
        let mut resp = req.call().map_err(|e| e.to_string())?;
        let status: u16 = resp.status().into();
        let body = resp.body_mut().read_to_string().unwrap_or_default();
        Ok((status, body))
    }

    /// PUT/POST with a body and arbitrary extra headers. Returns `(status, body)`.
    fn with_body(
        &self,
        method: WithBody,
        url: &str,
        content_type: &str,
        headers: &[(&str, String)],
        body: &[u8],
    ) -> Result<(u16, String), String> {
        let mut req = match method {
            WithBody::Put => self.agent.put(url),
            WithBody::Post => self.agent.post(url),
        };
        req = req.header("Content-Type", content_type);
        if let Some(h) = self.bearer() {
            req = req.header("Authorization", &h);
        }
        for (k, v) in headers {
            req = req.header(*k, v.as_str());
        }
        let mut resp = req.send(body).map_err(|e| e.to_string())?;
        let status: u16 = resp.status().into();
        let resp_body = resp.body_mut().read_to_string().unwrap_or_default();
        Ok((status, resp_body))
    }
}

fn ok_or(status: u16, body: String, what: &str) -> Result<String, String> {
    if (200..300).contains(&status) {
        Ok(body)
    } else if body.is_empty() {
        Err(format!("{what} failed: HTTP {status}"))
    } else {
        Err(format!("{what} failed (HTTP {status}): {body}"))
    }
}

#[async_trait::async_trait]
impl NamespaceProvider for HttpNamespaceProvider {
    async fn list_objects(&self, ns_id: &str) -> Result<Vec<ObjectMeta>, String> {
        let url = self.url(&format!(
            "/namespaces/{}/objects?limit=10000",
            Self::enc(ns_id)
        ));
        let (status, body) = self.no_body(NoBody::Get, &url)?;
        let body = ok_or(status, body, "list_objects")?;
        #[derive(serde::Deserialize)]
        struct Row {
            key: String,
            #[serde(default)]
            audience: Option<String>,
            #[serde(default)]
            content_hash: Option<String>,
        }
        let rows: Vec<Row> =
            serde_json::from_str(&body).map_err(|e| format!("list_objects decode: {e}"))?;
        Ok(rows
            .into_iter()
            .map(|r| ObjectMeta {
                key: r.key,
                audience: r.audience,
                content_hash: r.content_hash,
            })
            .collect())
    }

    async fn put_object(
        &self,
        ns_id: &str,
        key: &str,
        bytes: &[u8],
        mime_type: &str,
        audience: Option<&str>,
        file_ark: Option<&str>,
        source_key: Option<&str>,
        object_key: Option<&str>,
        is_index: bool,
    ) -> Result<(), String> {
        let url = self.url(&format!(
            "/namespaces/{}/objects/{}",
            Self::enc(ns_id),
            Self::enc_key(key)
        ));
        let mut headers: Vec<(&str, String)> = Vec::new();
        if let Some(a) = audience {
            headers.push(("X-Audience", a.to_string()));
        }
        if let Some(fa) = file_ark {
            headers.push(("X-Diaryx-File-Ark", fa.to_string()));
        }
        if let Some(sk) = source_key {
            headers.push(("X-Diaryx-Source-Key", sk.to_string()));
        }
        if let Some(ok) = object_key {
            headers.push(("X-Diaryx-Object-Key", ok.to_string()));
        }
        if is_index {
            headers.push(("X-Diaryx-Is-Index", "true".to_string()));
        }
        let (status, body) = self.with_body(WithBody::Put, &url, mime_type, &headers, bytes)?;
        ok_or(status, body, &format!("put_object {key}")).map(|_| ())
    }

    async fn delete_object(&self, ns_id: &str, key: &str) -> Result<(), String> {
        let url = self.url(&format!(
            "/namespaces/{}/objects/{}",
            Self::enc(ns_id),
            Self::enc_key(key)
        ));
        let (status, body) = self.no_body(NoBody::Delete, &url)?;
        ok_or(status, body, &format!("delete_object {key}")).map(|_| ())
    }

    async fn sync_audience(
        &self,
        ns_id: &str,
        audience: &str,
        gates: &yaml::Value,
    ) -> Result<(), String> {
        let url = self.url(&format!(
            "/namespaces/{}/audiences/{}",
            Self::enc(ns_id),
            Self::enc(audience)
        ));
        let body =
            serde_json::json!({ "gates": serde_json::Value::from(gates.clone()) }).to_string();
        let (status, resp) = self.with_body(
            WithBody::Put,
            &url,
            "application/json",
            &[],
            body.as_bytes(),
        )?;
        ok_or(status, resp, &format!("sync_audience {audience}")).map(|_| ())
    }

    async fn list_audiences(&self, ns_id: &str) -> Result<Vec<String>, String> {
        let url = self.url(&format!("/namespaces/{}/audiences", Self::enc(ns_id)));
        let (status, body) = self.no_body(NoBody::Get, &url)?;
        let body = ok_or(status, body, "list_audiences")?;
        #[derive(serde::Deserialize)]
        struct Item {
            name: String,
        }
        let items: Vec<Item> =
            serde_json::from_str(&body).map_err(|e| format!("list_audiences decode: {e}"))?;
        Ok(items.into_iter().map(|i| i.name).collect())
    }

    async fn delete_audience(&self, ns_id: &str, audience: &str) -> Result<(), String> {
        let url = self.url(&format!(
            "/namespaces/{}/audiences/{}",
            Self::enc(ns_id),
            Self::enc(audience)
        ));
        let (status, body) = self.no_body(NoBody::Delete, &url)?;
        ok_or(status, body, &format!("delete_audience {audience}")).map(|_| ())
    }

    async fn build_namespace(&self, ns_id: &str, base_url: Option<&str>) -> Result<(), String> {
        let mut path = format!("/namespaces/{}/build", Self::enc(ns_id));
        if let Some(bu) = base_url {
            path.push_str(&format!("?base_url={}", Self::enc(bu)));
        }
        let url = self.url(&path);
        let (status, body) = self.with_body(WithBody::Post, &url, "application/json", &[], &[])?;
        ok_or(status, body, "build_namespace").map(|_| ())
    }
}

// ---------------------------------------------------------------------------
// Command handlers
// ---------------------------------------------------------------------------

type Fs = SyncToAsyncFs<RealFileSystem>;

fn load_client(server: Option<&str>) -> Result<FsAuthenticatedClient, String> {
    let client = FsAuthenticatedClient::from_default_path(server)
        .ok_or("Cannot determine config directory for auth storage")?;
    if !block_on(client.has_session()) {
        return Err("Not logged in. Run `diaryx login <email>` first.".to_string());
    }
    Ok(client)
}

/// Mint a fresh `dx`-blade workspace id and register it as a namespace. The
/// server honors a client-supplied id verbatim, so the CLI controls the format
/// (the API otherwise mints UUIDs). Retries on the astronomically rare collision.
fn mint_and_create_namespace(client: &FsAuthenticatedClient) -> Result<String, String> {
    use diaryx_core::uuid::Uuid;
    for _ in 0..8 {
        let mut buf: Vec<u8> = Vec::new();
        let mut rng = || {
            if buf.is_empty() {
                buf.extend_from_slice(&Uuid::new_v4().into_bytes());
            }
            buf.pop().unwrap()
        };
        let blade = diaryx_ark::mint_workspace_blade(&mut rng);
        match block_on(namespace::create_namespace(client, Some(&blade), None)) {
            Ok(ns) => return Ok(ns.id),
            Err(e) if e.status_code == 409 => continue,
            Err(e) => return Err(format!("Failed to create namespace: {}", e.message)),
        }
    }
    Err("Could not mint a unique namespace id after 8 attempts".to_string())
}

/// Resolve the namespace id bound to a workspace's publish config, if any.
fn configured_namespace(ws: &Workspace<Fs>, root_index: &std::path::Path) -> Option<String> {
    block_on(ws.get_workspace_config(root_index))
        .ok()
        .and_then(|cfg| cfg.publish.and_then(|p| p.namespace_id))
}

#[allow(clippy::too_many_arguments)]
pub fn handle_publish(
    workspace: Option<PathBuf>,
    base_url: Option<String>,
    dry_run: bool,
    new_namespace: bool,
    json: bool,
    server: Option<String>,
) -> bool {
    let client = match load_client(server.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("✗ {e}");
            return false;
        }
    };

    let root_index = match resolve_workspace_for_export(workspace) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("✗ {e}");
            return false;
        }
    };

    let fs = SyncToAsyncFs::new(RealFileSystem);
    let ws = Workspace::new(fs.clone());

    // Dry run is strictly read-only: no config migration, no namespace mint, no
    // upload. Preview the plan against whatever namespace is already bound.
    if dry_run {
        let ns_id = match configured_namespace(&ws, &root_index) {
            Some(id) => id,
            None => {
                eprintln!(
                    "✗ No namespace bound to this workspace yet. Run `diaryx publish` (optionally --new-namespace) to create one; dry-run only previews against an existing namespace."
                );
                return false;
            }
        };
        let provider = HttpNamespaceProvider::from_client(&client);
        let service = PublishService::new(&provider);
        return match block_on(service.plan_workspace(fs.clone(), &root_index, &ns_id)) {
            Ok(plan) => {
                let summary = plan.to_summary_json();
                println!("{}", summary.to_json().unwrap_or_else(|_| "{}".to_string()));
                println!(
                    "\nDry run — nothing uploaded. Namespace: {ns_id} ({} upload(s), {} unchanged, {} delete(s)).",
                    plan.totals.uploads, plan.totals.unchanged, plan.totals.deletes
                );
                true
            }
            Err(e) => {
                eprintln!("✗ Publish plan failed: {e}");
                false
            }
        };
    }

    // Bring legacy publish config into the modern top-level `publish:` block
    // before reading/writing the namespace binding.
    match block_on(ws.migrate_publish_config(&root_index)) {
        Ok(true) => println!("✓ Migrated legacy publish config to top-level `publish:`"),
        Ok(false) => {}
        Err(e) => {
            eprintln!("✗ Failed to migrate publish config: {e}");
            return false;
        }
    }

    // Resolve (or mint) the namespace id = workspace ARK.
    let configured = configured_namespace(&ws, &root_index);
    let ns_id = if new_namespace || configured.is_none() {
        let blade = match mint_and_create_namespace(&client) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("✗ {e}");
                return false;
            }
        };
        // Persist the binding into the workspace settings, preserving any
        // existing publish fields (audiences, subdomain, …).
        let cfg = block_on(ws.get_workspace_config(&root_index)).unwrap_or_default();
        let mut publish = cfg.publish.unwrap_or_default();
        publish.namespace_id = Some(blade.clone());
        let value: yaml::Value = fig::ToValue::to_value(&publish).into();
        if let Err(e) = block_on(ws.set_workspace_config_field_value(&root_index, "publish", value))
        {
            eprintln!("✗ Failed to save namespace_id to workspace config: {e}");
            return false;
        }
        println!("✓ Created namespace {blade} and bound it to this workspace");
        blade
    } else {
        configured.unwrap()
    };

    let provider = HttpNamespaceProvider::from_client(&client);
    let service = PublishService::new(&provider);

    match block_on(service.publish_workspace(fs.clone(), &root_index, &ns_id, base_url.as_deref()))
    {
        Ok((plan, outcome)) => {
            if json {
                let out = serde_json::json!({
                    "namespace_id": ns_id,
                    "uploaded": outcome.uploaded,
                    "unchanged": plan.totals.unchanged,
                    "deleted": outcome.deleted,
                    "bytes_uploaded": outcome.bytes_uploaded,
                    "audiences_deleted": outcome.audiences_deleted,
                    "built": outcome.built,
                    "permalink": format!("https://diaryx.org/ark/{ns_id}/index"),
                });
                println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
            } else {
                println!(
                    "✓ Published to namespace {ns_id}: {} uploaded, {} unchanged, {} deleted ({} bytes).",
                    outcome.uploaded,
                    plan.totals.unchanged,
                    outcome.deleted,
                    outcome.bytes_uploaded
                );
                if !outcome.built {
                    eprintln!("⚠ Server-side render did not complete.");
                }
                println!("  Permalink: https://diaryx.org/ark/{ns_id}/index");
            }
            true
        }
        Err(e) => {
            eprintln!("✗ Publish failed: {e}");
            false
        }
    }
}

pub fn handle_unpublish(
    workspace: Option<PathBuf>,
    namespace_id: Option<String>,
    yes: bool,
    delete_namespace: bool,
    server: Option<String>,
) -> bool {
    let client = match load_client(server.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("✗ {e}");
            return false;
        }
    };

    // Namespace: explicit flag wins; otherwise read the workspace binding.
    let ns_id = match namespace_id {
        Some(id) => id,
        None => {
            let root_index = match resolve_workspace_for_export(workspace) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("✗ {e}");
                    return false;
                }
            };
            let ws = Workspace::new(SyncToAsyncFs::new(RealFileSystem));
            match configured_namespace(&ws, &root_index) {
                Some(id) => id,
                None => {
                    eprintln!("✗ No namespace bound to this workspace. Pass --namespace <id>.");
                    return false;
                }
            }
        }
    };

    if !yes {
        println!("Namespace: {ns_id}");
        println!(
            "This wipes ALL published objects in the namespace (the site goes blank){}.",
            if delete_namespace {
                " and deletes the namespace"
            } else {
                ""
            }
        );
        print!("Continue? [y/N] ");
        io::stdout().flush().ok();
        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            eprintln!("✗ Failed to read input");
            return false;
        }
        let input = input.trim().to_lowercase();
        if input != "y" && input != "yes" {
            println!("Cancelled.");
            return true;
        }
    }

    let provider = HttpNamespaceProvider::from_client(&client);
    let service = PublishService::new(&provider);
    let deleted = match block_on(service.unpublish_all(&ns_id)) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("✗ Unpublish failed: {e}");
            return false;
        }
    };
    println!("✓ Unpublished namespace {ns_id}: {deleted} object(s) deleted.");

    if delete_namespace {
        match block_on(namespace::delete_namespace(&client, &ns_id)) {
            Ok(()) => println!("✓ Deleted namespace {ns_id}."),
            Err(e) => {
                eprintln!("✗ Failed to delete namespace: {}", e.message);
                return false;
            }
        }
    }
    true
}

/// `namespace create [--id <blade>]` — register a namespace. Mints a fresh
/// `dx`-blade when `--id` is omitted.
pub fn handle_namespace_create(id: Option<String>) -> bool {
    let client = match load_client(None) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("✗ {e}");
            return false;
        }
    };
    let result = match id {
        Some(explicit) => block_on(namespace::create_namespace(&client, Some(&explicit), None))
            .map(|ns| ns.id)
            .map_err(|e| e.message),
        None => mint_and_create_namespace(&client),
    };
    match result {
        Ok(ns_id) => {
            println!("✓ Created namespace {ns_id}");
            true
        }
        Err(e) => {
            eprintln!("✗ Failed to create namespace: {e}");
            false
        }
    }
}
