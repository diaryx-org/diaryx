/**
 * Namespace Service — API client for namespace management operations.
 *
 * Communicates directly with the sync server via proxyFetch, removing
 * the need for plugins to round-trip through WASM for namespace CRUD.
 */

import { getServerUrl } from '$lib/auth';
import { proxyFetch } from '$lib/backend/proxyFetch';

// ============================================================================
// Types
// ============================================================================

export interface NamespaceInfo {
  id: string;
  owner_user_id: string;
  created_at: number;
}

export interface AudienceInfo {
  name: string;
  access: string;
}

export interface SubdomainInfo {
  subdomain: string;
  namespace_id: string;
}

export interface DomainInfo {
  domain: string;
  namespace_id: string;
  audience_name: string;
  created_at: number;
  verified: boolean;
}

export interface TokenResult {
  token: string;
}

// ============================================================================
// API Helpers
// ============================================================================

function getApiBase(): { serverUrl: string } | null {
  const serverUrl = getServerUrl();
  if (!serverUrl) return null;
  return { serverUrl: serverUrl.replace(/\/$/, '') };
}

async function apiFetch<T>(
  path: string,
  options?: RequestInit,
): Promise<T> {
  const base = getApiBase();
  if (!base) throw new Error('Not authenticated');

  const response = await proxyFetch(
    `${base.serverUrl}${path}`,
    {
      ...options,
      headers: {
        'Content-Type': 'application/json',
        ...options?.headers,
      },
    },
  );

  if (!response.ok) {
    const text = await response.text();
    let message = text || `Request failed: ${response.status}`;
    try {
      const parsed = JSON.parse(text);
      if (parsed.error) message = parsed.error;
    } catch { /* use raw text */ }
    throw new Error(message);
  }

  // Handle 204 No Content
  if (response.status === 204) return undefined as T;
  return response.json();
}

// ============================================================================
// Public API
// ============================================================================

export async function createNamespace(id?: string): Promise<NamespaceInfo> {
  return apiFetch<NamespaceInfo>('/namespaces', {
    method: 'POST',
    body: JSON.stringify(id ? { id } : {}),
  });
}

export async function deleteNamespace(nsId: string): Promise<void> {
  await apiFetch<void>(
    `/namespaces/${encodeURIComponent(nsId)}`,
    { method: 'DELETE' },
  );
}

export async function listAudiences(nsId: string): Promise<AudienceInfo[]> {
  return apiFetch<AudienceInfo[]>(
    `/namespaces/${encodeURIComponent(nsId)}/audiences`,
  );
}

export async function setAudience(
  nsId: string,
  name: string,
  access: string,
): Promise<void> {
  await apiFetch<void>(
    `/namespaces/${encodeURIComponent(nsId)}/audiences/${encodeURIComponent(name)}`,
    {
      method: 'PUT',
      body: JSON.stringify({ access }),
    },
  );
}

export async function getAudienceToken(
  nsId: string,
  name: string,
): Promise<TokenResult> {
  return apiFetch<TokenResult>(
    `/namespaces/${encodeURIComponent(nsId)}/audiences/${encodeURIComponent(name)}/token`,
  );
}

export async function claimSubdomain(
  nsId: string,
  subdomain: string,
  defaultAudience?: string,
): Promise<SubdomainInfo> {
  const body: Record<string, string> = { subdomain };
  if (defaultAudience) body.default_audience = defaultAudience;
  return apiFetch<SubdomainInfo>(
    `/namespaces/${encodeURIComponent(nsId)}/subdomain`,
    {
      method: 'PUT',
      body: JSON.stringify(body),
    },
  );
}

export async function releaseSubdomain(nsId: string): Promise<void> {
  await apiFetch<void>(
    `/namespaces/${encodeURIComponent(nsId)}/subdomain`,
    { method: 'DELETE' },
  );
}

export async function listDomains(nsId: string): Promise<DomainInfo[]> {
  return apiFetch<DomainInfo[]>(
    `/namespaces/${encodeURIComponent(nsId)}/domains`,
  );
}

export async function registerDomain(
  nsId: string,
  domain: string,
  audienceName: string,
): Promise<DomainInfo> {
  return apiFetch<DomainInfo>(
    `/namespaces/${encodeURIComponent(nsId)}/domains/${encodeURIComponent(domain)}`,
    {
      method: 'PUT',
      body: JSON.stringify({ audience_name: audienceName }),
    },
  );
}

export async function removeDomain(
  nsId: string,
  domain: string,
): Promise<void> {
  await apiFetch<void>(
    `/namespaces/${encodeURIComponent(nsId)}/domains/${encodeURIComponent(domain)}`,
    { method: 'DELETE' },
  );
}

// ============================================================================
// Subscribers
// ============================================================================

export interface SubscriberInfo {
  id: string;
  email: string;
}

export interface BulkImportResult {
  added: number;
  errors: string[];
}

export async function listSubscribers(
  nsId: string,
  audienceName: string,
): Promise<SubscriberInfo[]> {
  return apiFetch<SubscriberInfo[]>(
    `/namespaces/${encodeURIComponent(nsId)}/audiences/${encodeURIComponent(audienceName)}/subscribers`,
  );
}

export async function addSubscriber(
  nsId: string,
  audienceName: string,
  email: string,
): Promise<SubscriberInfo> {
  return apiFetch<SubscriberInfo>(
    `/namespaces/${encodeURIComponent(nsId)}/audiences/${encodeURIComponent(audienceName)}/subscribers`,
    {
      method: 'POST',
      body: JSON.stringify({ email }),
    },
  );
}

export async function removeSubscriber(
  nsId: string,
  audienceName: string,
  contactId: string,
): Promise<void> {
  await apiFetch<void>(
    `/namespaces/${encodeURIComponent(nsId)}/audiences/${encodeURIComponent(audienceName)}/subscribers/${encodeURIComponent(contactId)}`,
    { method: 'DELETE' },
  );
}

export async function bulkImportSubscribers(
  nsId: string,
  audienceName: string,
  emails: string[],
): Promise<BulkImportResult> {
  return apiFetch<BulkImportResult>(
    `/namespaces/${encodeURIComponent(nsId)}/audiences/${encodeURIComponent(audienceName)}/subscribers/import`,
    {
      method: 'POST',
      body: JSON.stringify({ emails }),
    },
  );
}

/**
 * Build a subscriber signup URL for an audience.
 */
export function buildSubscribeUrl(
  nsId: string,
  audienceName: string,
): string {
  const base = getApiBase();
  if (!base) return '';
  return `${base.serverUrl}/namespaces/${encodeURIComponent(nsId)}/audiences/${encodeURIComponent(audienceName)}/subscribers`;
}

/**
 * Check if the namespace API is available (user is authenticated with a server).
 */
export function isNamespaceAvailable(): boolean {
  return getApiBase() !== null;
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
