/**
 * Site Publishing Service — API client for workspace site publishing.
 */

import { getToken, getServerUrl } from '$lib/auth';

// ============================================================================
// Types
// ============================================================================

export interface PublishedSite {
  id: string;
  workspace_id: string;
  slug: string;
  enabled: boolean;
  auto_publish: boolean;
  last_published_at: number | null;
  created_at: number;
  updated_at: number;
}

export interface AudienceBuildSummary {
  name: string;
  file_count: number;
  built_at: number;
}

export interface PublishAudienceSummary {
  name: string;
  file_count: number;
}

export interface PublishResult {
  slug: string;
  audiences: PublishAudienceSummary[];
  published_at: number;
}

export interface SiteAccessToken {
  id: string;
  audience: string;
  label: string | null;
  expires_at: number | null;
  revoked: boolean;
  created_at: number;
}

export interface CreateSiteRequest {
  slug: string;
  auto_publish?: boolean;
  enabled?: boolean;
}

export interface CreateTokenRequest {
  audience: string;
  label?: string;
  expires_in?: string | null;
}

export interface ApiError extends Error {
  status: number;
  code?: string;
  details?: unknown;
}

interface SiteResponse extends PublishedSite {
  audiences: AudienceBuildSummary[];
}

interface ErrorPayload {
  error?: string;
  message?: string;
}

interface CreateTokenResponse {
  id: string;
  audience: string;
  label: string | null;
  expires_at: number | null;
  created_at: number;
  access_url: string;
}

// ============================================================================
// API Helpers
// ============================================================================

function getApiBase(workspaceId: string): { serverUrl: string; token: string; workspaceId: string } | null {
  const serverUrl = getServerUrl();
  const token = getToken();
  if (!serverUrl || !token || !workspaceId) return null;
  return {
    serverUrl: serverUrl.replace(/\/$/, ''),
    token,
    workspaceId,
  };
}

function createApiError(status: number, code: string | undefined, details: unknown): ApiError {
  const message = mapErrorMessage(status, code, details);
  const error = new Error(message) as ApiError;
  error.status = status;
  error.code = code;
  error.details = details;
  return error;
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

function mapErrorMessage(status: number, code: string | undefined, details: unknown): string {
  const payload = isObject(details) ? details : null;
  const backendMessage = typeof payload?.message === 'string' ? payload.message : null;

  if (status === 400) {
    return backendMessage ?? 'Invalid request. Check slug, audience, and expiration values.';
  }

  if (status === 403) {
    if (code === 'site_limit_reached') {
      return 'Published site limit reached for this account.';
    }
    return backendMessage ?? 'You are not allowed to perform this publishing action.';
  }

  if (status === 404) {
    return backendMessage ?? 'Published site not found for this workspace.';
  }

  if (status === 409) {
    if (code === 'publish_in_progress') {
      return 'A publish is already in progress for this workspace.';
    }
    if (code === 'slug_conflict') {
      return 'Slug already exists. Choose another slug.';
    }
    if (code === 'site_exists') {
      return 'This workspace already has a configured site.';
    }
    return backendMessage ?? 'Conflict while processing publishing request.';
  }

  if (status >= 500 && backendMessage) {
    const normalized = backendMessage.toLowerCase();
    if (
      normalized.includes('workspace has no materialized markdown files')
      || normalized.includes('failed to open workspace storage')
    ) {
      return 'Sync must be enabled and completed at least once before publishing this workspace.';
    }
  }

  if (backendMessage) {
    return backendMessage;
  }

  return `Publishing request failed (${status}).`;
}

async function parseErrorPayload(response: Response): Promise<ErrorPayload | null> {
  const contentType = response.headers.get('content-type') || '';
  if (!contentType.includes('application/json')) return null;

  try {
    return await response.json();
  } catch {
    return null;
  }
}

async function apiFetch<T>(workspaceId: string, path: string, options?: RequestInit): Promise<T> {
  const base = getApiBase(workspaceId);
  if (!base) {
    throw createApiError(401, 'not_authenticated', {
      message: 'Not authenticated or missing workspace.',
    });
  }

  const response = await fetch(
    `${base.serverUrl}/api/workspaces/${encodeURIComponent(base.workspaceId)}${path}`,
    {
      ...options,
      headers: {
        Authorization: `Bearer ${base.token}`,
        'Content-Type': 'application/json',
        ...options?.headers,
      },
    },
  );

  if (!response.ok) {
    const payload = await parseErrorPayload(response);
    throw createApiError(response.status, payload?.error, payload ?? undefined);
  }

  if (response.status === 204) {
    return undefined as T;
  }

  return response.json();
}

function toPublishedSite(site: SiteResponse): PublishedSite {
  return {
    id: site.id,
    workspace_id: site.workspace_id,
    slug: site.slug,
    enabled: site.enabled,
    auto_publish: site.auto_publish,
    last_published_at: site.last_published_at,
    created_at: site.created_at,
    updated_at: site.updated_at,
  };
}

function toAccessToken(token: CreateTokenResponse | SiteAccessToken): SiteAccessToken {
  return {
    id: token.id,
    audience: token.audience,
    label: token.label,
    expires_at: token.expires_at,
    revoked: 'revoked' in token ? token.revoked : false,
    created_at: token.created_at,
  };
}

// ============================================================================
// Public API
// ============================================================================

export async function getSite(
  workspaceId: string,
): Promise<{ site: PublishedSite; audiences: AudienceBuildSummary[] } | null> {
  try {
    const response = await apiFetch<SiteResponse>(workspaceId, '/site');
    return {
      site: toPublishedSite(response),
      audiences: response.audiences,
    };
  } catch (error) {
    const apiError = error as ApiError;
    if (apiError.status === 404) {
      return null;
    }
    throw error;
  }
}

export async function createSite(workspaceId: string, input: CreateSiteRequest): Promise<PublishedSite> {
  const response = await apiFetch<SiteResponse>(workspaceId, '/site', {
    method: 'POST',
    body: JSON.stringify(input),
  });
  return toPublishedSite(response);
}

export async function deleteSite(workspaceId: string): Promise<void> {
  await apiFetch<void>(workspaceId, '/site', {
    method: 'DELETE',
  });
}

export async function publishSite(workspaceId: string): Promise<PublishResult> {
  return apiFetch<PublishResult>(workspaceId, '/site/publish', {
    method: 'POST',
  });
}

export async function createToken(
  workspaceId: string,
  input: CreateTokenRequest,
): Promise<{ token: SiteAccessToken; accessUrl: string }> {
  const response = await apiFetch<CreateTokenResponse>(workspaceId, '/site/tokens', {
    method: 'POST',
    body: JSON.stringify(input),
  });

  return {
    token: toAccessToken(response),
    accessUrl: response.access_url,
  };
}

export async function listTokens(workspaceId: string): Promise<SiteAccessToken[]> {
  const response = await apiFetch<SiteAccessToken[]>(workspaceId, '/site/tokens');
  return response.map((token) => toAccessToken(token));
}

export async function revokeToken(workspaceId: string, tokenId: string): Promise<void> {
  await apiFetch<void>(workspaceId, `/site/tokens/${encodeURIComponent(tokenId)}`, {
    method: 'DELETE',
  });
}

export function isSitePublishingAvailable(workspaceId: string | null | undefined): boolean {
  return getApiBase(workspaceId ?? '') !== null;
}
