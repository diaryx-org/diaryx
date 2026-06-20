/**
 * corePublishService — drives `diaryx_core::publish` directly, with NO Extism
 * `diaryx.publish` plugin.
 *
 * - **Browser**: the WASM backend worker runs the publish algorithm over the
 *   workspace filesystem; namespace HTTP runs on the main thread through a
 *   `Comlink.proxy`'d `PublishProvider` (see `publishProvider.ts`).
 * - **Tauri**: the `publish_to_namespace` / `preview_to_namespace` IPC commands
 *   reuse the keyring-backed client as the `NamespaceProvider`.
 *
 * Publish config (namespace id, subdomain, per-audience access state) lives in
 * the root index frontmatter under `plugins."diaryx.publish"` — the same
 * location the plugin used. It is read/written through the core
 * `GetPluginConfig` / `SetPluginConfig` commands (plugin-independent), so
 * existing workspaces keep working after the plugin is removed.
 */

import * as Comlink from "comlink";
import { isTauri } from "$lib/backend";
import { getBackend } from "$lib/backend";
import type { WorkerBackendNew } from "$lib/backend/workerBackendNew";
import type { WorkerApi } from "$lib/backend/wasmWorkerNew";
import type { Api } from "$lib/backend/api";
import type { JsonValue } from "$lib/backend/generated/serde_json/JsonValue";
import { createPublishProvider } from "./publishProvider";

// ============================================================================
// Config types (mirror the Rust PublishPluginConfig shape)
// ============================================================================

export type AudienceConfig = { state: string; access_method?: string };

export interface PublishConfig {
  namespace_id?: string | null;
  subdomain?: string | null;
  audience_states?: Record<string, AudienceConfig>;
  public_audiences?: string[];
  [key: string]: unknown;
}

// ============================================================================
// Worker remote (browser path)
// ============================================================================

async function getWorkerRemote(): Promise<Comlink.Remote<WorkerApi>> {
  const backend = await getBackend();
  const maybeWorkerBackend = backend as unknown as Partial<WorkerBackendNew>;
  const getWorkerApi = maybeWorkerBackend.getWorkerApi?.bind(backend);
  const remote = getWorkerApi ? getWorkerApi() : null;
  if (!remote) {
    throw new Error(
      "Browser backend is not using the WASM worker — publish is unavailable.",
    );
  }
  return remote as Comlink.Remote<WorkerApi>;
}

async function tauriInvoke<T>(cmd: string, args: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

// ============================================================================
// Publish + preview
// ============================================================================

/** Run a publish (or preview) of the active workspace's namespace. */
async function runPublish(
  serverUrl: string,
  namespaceId: string,
  baseUrl: string | null,
  preview: boolean,
): Promise<any> {
  if (isTauri()) {
    const cmd = preview ? "preview_to_namespace" : "publish_to_namespace";
    const args: Record<string, unknown> = { namespaceId };
    if (!preview) args.baseUrl = baseUrl ?? null;
    return tauriInvoke<any>(cmd, args);
  }

  const remote = await getWorkerRemote();
  const provider = Comlink.proxy(createPublishProvider(serverUrl));
  const json = await remote.publishWorkspace(
    provider as unknown,
    namespaceId,
    baseUrl ?? null,
    preview,
  );
  return JSON.parse(json);
}

export function previewPublish(
  serverUrl: string,
  namespaceId: string,
): Promise<any> {
  return runPublish(serverUrl, namespaceId, null, true);
}

export function publishToNamespace(
  serverUrl: string,
  namespaceId: string,
  baseUrl: string | null = null,
): Promise<any> {
  return runPublish(serverUrl, namespaceId, baseUrl, false);
}

// ============================================================================
// Config (frontmatter `plugins."diaryx.publish"`, via core Get/SetPluginConfig)
// ============================================================================

const PUBLISH_PLUGIN_ID = "diaryx.publish";

async function rootIndexPath(api: Api): Promise<string> {
  const root = await api.resolveWorkspaceRootIndexPath();
  if (!root) throw new Error("No workspace root index");
  return root;
}

export async function getPublishConfig(api: Api): Promise<PublishConfig> {
  const root = await rootIndexPath(api);
  const raw = (await api.getWorkspacePluginData(
    root,
    PUBLISH_PLUGIN_ID,
  )) as PublishConfig | null;
  return raw ?? {};
}

export async function setPublishConfig(
  api: Api,
  config: PublishConfig,
): Promise<void> {
  // Merge over the persisted config so unrelated keys survive.
  const root = await rootIndexPath(api);
  const existing = await getPublishConfig(api);
  await api.setWorkspacePluginData(root, PUBLISH_PLUGIN_ID, {
    ...existing,
    ...config,
  } as unknown as JsonValue);
}

/**
 * Persist a single audience's publish state into the config. The server-side
 * gate sync already happens in the audience-management UI (via
 * `coreNamespaceService`); this is the best-effort frontmatter mirror that the
 * panel keeps for its own state.
 */
export async function setAudiencePublishState(
  api: Api,
  audience: string,
  config: AudienceConfig,
): Promise<Record<string, AudienceConfig>> {
  const current = await getPublishConfig(api);
  const audienceStates: Record<string, AudienceConfig> = {
    ...(current.audience_states ?? {}),
  };
  const publicAudiences = new Set(current.public_audiences ?? []);

  if (config.state === "unpublished") {
    delete audienceStates[audience];
    publicAudiences.delete(audience);
  } else {
    audienceStates[audience] = config;
    if (config.state === "public") publicAudiences.add(audience);
    else publicAudiences.delete(audience);
  }

  await setPublishConfig(api, {
    ...current,
    audience_states: audienceStates,
    public_audiences: [...publicAudiences],
  });
  return audienceStates;
}
