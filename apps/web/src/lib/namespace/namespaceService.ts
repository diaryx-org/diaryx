/**
 * Namespace Service — thin facade over the `coreNamespaceService` router.
 *
 * Historically this module held a hand-written `proxyFetch` HTTP client for
 * the sync server's `/namespaces` endpoints. That logic now lives in Rust
 * (`diaryx_core::namespace`) and is exposed per-platform through:
 *
 * - `wasmNamespaceService.ts` → worker-hosted `NamespaceClient` (browser)
 * - `tauriNamespaceService.ts` → `namespace_*` IPC commands (Tauri/macOS/iOS)
 *
 * These are multiplexed by `coreNamespaceRouter.ts`.
 *
 * The function exports here stay as-is for backward compatibility with the
 * existing callers (`import * as namespaceService from './namespaceService'`
 * or `import { createNamespace, … } from …`). The pure URL helpers at the
 * bottom (`buildAccessUrl`, `isNamespaceAvailable`) stay because they don't
 * talk to the server.
 */

import { getServerUrl } from "$lib/auth";
import { coreNamespaceService } from "./coreNamespaceRouter";
import type {
  NamespaceInfo,
  AudienceInfo,
  SubdomainInfo,
  DomainInfo,
  TokenResult,
  RotatePasswordResult,
} from "./coreNamespaceTypes";

// ============================================================================
// Types (re-exported for existing callers; source of truth lives in
// `coreNamespaceTypes.ts`, which in turn mirrors `diaryx_core::namespace`).
// ============================================================================

export type {
  NamespaceInfo,
  AudienceInfo,
  SubdomainInfo,
  DomainInfo,
  TokenResult,
  RotatePasswordResult,
} from "./coreNamespaceTypes";

// ============================================================================
// Public API — thin delegates to `coreNamespaceService`.
// ============================================================================

export async function createNamespace(
  id?: string,
  metadata?: Record<string, unknown> | null,
): Promise<NamespaceInfo> {
  return coreNamespaceService.createNamespace(id ?? null, metadata);
}

export async function updateNamespaceMetadata(
  nsId: string,
  metadata: Record<string, unknown> | null,
): Promise<NamespaceInfo> {
  return coreNamespaceService.updateNamespaceMetadata(nsId, metadata);
}

export async function deleteNamespace(nsId: string): Promise<void> {
  return coreNamespaceService.deleteNamespace(nsId);
}

export async function listAudiences(nsId: string): Promise<AudienceInfo[]> {
  return coreNamespaceService.listAudiences(nsId);
}

/**
 * Legacy access-string overload retained for the old "Set audience access"
 * UI path. Newer flows source audiences from the workspace file
 * (`WorkspaceConfig.audiences`) and sync them on publish; this stays so the
 * pre-Step-F UI keeps working until callers move over.
 */
export async function setAudience(
  nsId: string,
  name: string,
  access: string,
): Promise<void> {
  return coreNamespaceService.setAudience(nsId, name, access);
}

export async function getAudienceToken(
  nsId: string,
  name: string,
): Promise<TokenResult> {
  return coreNamespaceService.getAudienceToken(nsId, name);
}

/**
 * Set or rotate the password on an audience's password gate. Returns the
 * new gate version + a fresh unlock token. Old unlock tokens minted under
 * any previous version stop validating immediately.
 *
 * The audience must already declare a `password` gate in the workspace
 * file's `audiences:` block and have been synced to the server (i.e. the
 * writer has published at least once after declaring it). Calling against
 * an audience without a password gate will throw.
 */
export async function rotateAudiencePassword(
  nsId: string,
  name: string,
  password: string,
): Promise<RotatePasswordResult> {
  return coreNamespaceService.rotateAudiencePassword(nsId, name, password);
}

export async function claimSubdomain(
  nsId: string,
  subdomain: string,
  defaultAudience?: string,
): Promise<SubdomainInfo> {
  return coreNamespaceService.claimSubdomain(
    nsId,
    subdomain,
    defaultAudience ?? null,
  );
}

export async function releaseSubdomain(nsId: string): Promise<void> {
  return coreNamespaceService.releaseSubdomain(nsId);
}

export async function listDomains(nsId: string): Promise<DomainInfo[]> {
  return coreNamespaceService.listDomains(nsId);
}

export async function registerDomain(
  nsId: string,
  domain: string,
  audienceName: string,
): Promise<DomainInfo> {
  return coreNamespaceService.registerDomain(nsId, domain, audienceName);
}

export async function removeDomain(
  nsId: string,
  domain: string,
): Promise<void> {
  return coreNamespaceService.removeDomain(nsId, domain);
}

// ============================================================================
// Pure URL helpers — no server round-trip, stay in TS
// ============================================================================

/**
 * Check if the namespace API is available (user is authenticated with a server).
 */
export function isNamespaceAvailable(): boolean {
  return getServerUrl() !== null;
}

/**
 * Build an access URL for a namespace audience.
 *
 * @param siteBaseUrl - Server's site base URL (from capabilities), e.g. "http://localhost:3030"
 * @param siteDomain - Domain for subdomain routing (e.g. "diaryx.org"), null if unavailable
 */
export function buildAccessUrl(
  nsId: string,
  audience: string,
  token?: string,
  subdomain?: string,
  siteBaseUrl?: string | null,
  siteDomain?: string | null,
): string {
  let base: string;
  if (subdomain && siteDomain) {
    // Subdomain routing via Caddy or Cloudflare
    base = `https://${subdomain}.${siteDomain}/${encodeURIComponent(audience)}/index.html`;
  } else if (siteBaseUrl) {
    // Direct serving from sync server
    base = `${siteBaseUrl}/sites/${encodeURIComponent(nsId)}/${encodeURIComponent(audience)}/index.html`;
  } else {
    // Fallback
    base = `/sites/${encodeURIComponent(nsId)}/${encodeURIComponent(audience)}/index.html`;
  }

  if (token) {
    return `${base}?audience_token=${encodeURIComponent(token)}`;
  }
  return base;
}
