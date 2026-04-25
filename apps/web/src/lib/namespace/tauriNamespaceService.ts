/**
 * tauriNamespaceService — Typed wrappers around the Tauri `namespace_*` IPC
 * commands.
 *
 * These live in `apps/tauri/src-tauri/src/namespace_commands.rs` and reuse
 * the keyring-backed `AuthService` session so the token never crosses IPC.
 * Errors surface as the same `SerializableAuthError` the auth IPC commands
 * use, re-wrapped into `AuthError`.
 */

import { invoke } from "@tauri-apps/api/core";
import { AuthError } from "$lib/auth/coreAuthTypes";
import type {
  CoreNamespaceService,
  NamespaceInfo,
  AudienceInfo,
  SubdomainInfo,
  DomainInfo,
  TokenResult,
  RotatePasswordResult,
  SubscriberInfo,
  BulkImportResult,
} from "./coreNamespaceTypes";

function wrapError(err: unknown): AuthError {
  if (err instanceof AuthError) return err;
  if (err && typeof err === "object") {
    const o = err as {
      message?: string;
      statusCode?: number;
      status_code?: number;
    };
    const status = o.statusCode ?? o.status_code ?? 0;
    return new AuthError(o.message ?? "Namespace request failed", status);
  }
  if (typeof err === "string") {
    return new AuthError(err, 0);
  }
  return new AuthError("Namespace request failed", 0);
}

async function call<T>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (err) {
    throw wrapError(err);
  }
}

/**
 * Create a `CoreNamespaceService` backed by Tauri IPC commands.
 */
export function createTauriNamespaceService(): CoreNamespaceService {
  return {
    getNamespace(id) {
      return call<NamespaceInfo>("namespace_get", { id });
    },

    createNamespace(id, metadata) {
      return call<NamespaceInfo>("namespace_create", {
        id: id ?? null,
        metadata: metadata ?? null,
      });
    },

    updateNamespaceMetadata(id, metadata) {
      return call<NamespaceInfo>("namespace_update_metadata", {
        id,
        metadata,
      });
    },

    async deleteNamespace(id) {
      await call<void>("namespace_delete", { id });
    },

    listAudiences(id) {
      return call<AudienceInfo[]>("namespace_list_audiences", { id });
    },

    async setAudience(id, name, access) {
      await call<void>("namespace_set_audience", { id, name, access });
    },

    getAudienceToken(id, name) {
      return call<TokenResult>("namespace_get_audience_token", { id, name });
    },

    rotateAudiencePassword(id, name, password) {
      return call<RotatePasswordResult>(
        "namespace_rotate_audience_password",
        { id, name, password },
      );
    },

    claimSubdomain(id, subdomain, defaultAudience) {
      return call<SubdomainInfo>("namespace_claim_subdomain", {
        id,
        subdomain,
        defaultAudience: defaultAudience ?? null,
      });
    },

    async releaseSubdomain(id) {
      await call<void>("namespace_release_subdomain", { id });
    },

    listDomains(id) {
      return call<DomainInfo[]>("namespace_list_domains", { id });
    },

    registerDomain(id, domain, audienceName) {
      return call<DomainInfo>("namespace_register_domain", {
        id,
        domain,
        audienceName,
      });
    },

    async removeDomain(id, domain) {
      await call<void>("namespace_remove_domain", { id, domain });
    },

    listSubscribers(id, audience) {
      return call<SubscriberInfo[]>("namespace_list_subscribers", {
        id,
        audience,
      });
    },

    addSubscriber(id, audience, email) {
      return call<SubscriberInfo>("namespace_add_subscriber", {
        id,
        audience,
        email,
      });
    },

    async removeSubscriber(id, audience, contactId) {
      await call<void>("namespace_remove_subscriber", {
        id,
        audience,
        contactId,
      });
    },

    bulkImportSubscribers(id, audience, emails) {
      return call<BulkImportResult>("namespace_bulk_import_subscribers", {
        id,
        audience,
        emails,
      });
    },
  };
}
