/**
 * Site Publishing Store - Manages published-site state in the Share sidebar.
 */

import { getDefaultWorkspace } from '$lib/auth';
import {
  createSite,
  createToken,
  deleteSite,
  getSite,
  listTokens,
  publishSite,
  revokeToken,
  type AudienceBuildSummary,
  type CreateSiteRequest,
  type CreateTokenRequest,
  type PublishedSite,
  type SiteAccessToken,
} from '../services/sitePublishingService';

// ============================================================================
// Helpers
// ============================================================================

function getErrorMessage(error: unknown): string {
  if (error instanceof Error && error.message) return error.message;
  return 'Publishing action failed.';
}

// ============================================================================
// Store
// ============================================================================

class SitePublishingStore {
  site = $state<PublishedSite | null>(null);
  audiences = $state<AudienceBuildSummary[]>([]);
  tokens = $state<SiteAccessToken[]>([]);

  error = $state<string | null>(null);
  lastPublishedAt = $state<number | null>(null);
  lastCreatedAccessUrl = $state<string | null>(null);

  isLoading = $state(false);
  isCreatingSite = $state(false);
  isDeletingSite = $state(false);
  isPublishing = $state(false);
  isCreatingToken = $state(false);
  isRevokingToken = $state(false);
  isRefreshingTokens = $state(false);

  get state() {
    return {
      site: this.site,
      audiences: this.audiences,
      tokens: this.tokens,
      error: this.error,
      lastPublishedAt: this.lastPublishedAt,
      lastCreatedAccessUrl: this.lastCreatedAccessUrl,
      loading: {
        load: this.isLoading,
        createSite: this.isCreatingSite,
        deleteSite: this.isDeletingSite,
        publish: this.isPublishing,
        createToken: this.isCreatingToken,
        revokeToken: this.isRevokingToken,
        refreshTokens: this.isRefreshingTokens,
      },
    };
  }

  get defaultWorkspaceId(): string | null {
    return getDefaultWorkspace()?.id ?? null;
  }

  get hasDefaultWorkspace(): boolean {
    return this.defaultWorkspaceId !== null;
  }

  get isConfigured(): boolean {
    return this.site !== null;
  }

  get canPublish(): boolean {
    return (
      this.hasDefaultWorkspace
      && this.site !== null
      && !this.isPublishing
      && !this.isLoading
      && !this.isCreatingSite
      && !this.isDeletingSite
    );
  }

  clearLastCreatedAccessUrl() {
    this.lastCreatedAccessUrl = null;
  }

  clearError() {
    this.error = null;
  }

  private resolveWorkspaceId(workspaceId?: string): string | null {
    const resolved = workspaceId ?? this.defaultWorkspaceId;
    if (!resolved) {
      this.error = 'No default workspace is available for publishing.';
      return null;
    }
    return resolved;
  }

  async load(workspaceId?: string): Promise<void> {
    if (this.isLoading) return;

    const resolvedWorkspaceId = this.resolveWorkspaceId(workspaceId);
    if (!resolvedWorkspaceId) {
      this.site = null;
      this.audiences = [];
      this.tokens = [];
      this.lastPublishedAt = null;
      return;
    }

    this.isLoading = true;
    this.error = null;

    try {
      const response = await getSite(resolvedWorkspaceId);

      if (!response) {
        this.site = null;
        this.audiences = [];
        this.tokens = [];
        this.lastPublishedAt = null;
        this.lastCreatedAccessUrl = null;
        return;
      }

      this.site = response.site;
      this.audiences = response.audiences;
      this.lastPublishedAt = response.site.last_published_at;

      await this.refreshTokens(resolvedWorkspaceId);
    } catch (error) {
      this.error = getErrorMessage(error);
      console.error('[SitePublishingStore] Failed to load publishing state:', error);
    } finally {
      this.isLoading = false;
    }
  }

  async create(input: CreateSiteRequest, workspaceId?: string): Promise<PublishedSite | null> {
    if (this.isCreatingSite) return null;

    const resolvedWorkspaceId = this.resolveWorkspaceId(workspaceId);
    if (!resolvedWorkspaceId) return null;

    this.isCreatingSite = true;
    this.error = null;

    try {
      const site = await createSite(resolvedWorkspaceId, input);
      this.site = site;
      this.audiences = [];
      this.tokens = [];
      this.lastPublishedAt = site.last_published_at;
      this.lastCreatedAccessUrl = null;
      await this.refreshTokens(resolvedWorkspaceId);
      return site;
    } catch (error) {
      this.error = getErrorMessage(error);
      console.error('[SitePublishingStore] Failed to create site:', error);
      return null;
    } finally {
      this.isCreatingSite = false;
    }
  }

  async remove(workspaceId?: string): Promise<boolean> {
    if (this.isDeletingSite) return false;

    const resolvedWorkspaceId = this.resolveWorkspaceId(workspaceId);
    if (!resolvedWorkspaceId) return false;

    this.isDeletingSite = true;
    this.error = null;

    try {
      await deleteSite(resolvedWorkspaceId);
      this.site = null;
      this.audiences = [];
      this.tokens = [];
      this.lastPublishedAt = null;
      this.lastCreatedAccessUrl = null;
      return true;
    } catch (error) {
      this.error = getErrorMessage(error);
      console.error('[SitePublishingStore] Failed to delete site:', error);
      return false;
    } finally {
      this.isDeletingSite = false;
    }
  }

  async publishNow(workspaceId?: string): Promise<boolean> {
    if (this.isPublishing) return false;

    const resolvedWorkspaceId = this.resolveWorkspaceId(workspaceId);
    if (!resolvedWorkspaceId) return false;

    this.isPublishing = true;
    this.error = null;

    try {
      const result = await publishSite(resolvedWorkspaceId);
      this.lastPublishedAt = result.published_at;

      if (this.site) {
        this.site = {
          ...this.site,
          last_published_at: result.published_at,
          updated_at: result.published_at,
        };
      }

      this.audiences = result.audiences.map((audience) => ({
        name: audience.name,
        file_count: audience.file_count,
        built_at: result.published_at,
      }));

      return true;
    } catch (error) {
      this.error = getErrorMessage(error);
      console.error('[SitePublishingStore] Failed to publish site:', error);
      return false;
    } finally {
      this.isPublishing = false;
    }
  }

  async createToken(input: CreateTokenRequest, workspaceId?: string): Promise<SiteAccessToken | null> {
    if (this.isCreatingToken) return null;

    const resolvedWorkspaceId = this.resolveWorkspaceId(workspaceId);
    if (!resolvedWorkspaceId) return null;

    this.isCreatingToken = true;
    this.error = null;

    try {
      const result = await createToken(resolvedWorkspaceId, input);
      this.tokens = [result.token, ...this.tokens.filter((token) => token.id !== result.token.id)];
      this.lastCreatedAccessUrl = result.accessUrl;
      return result.token;
    } catch (error) {
      this.error = getErrorMessage(error);
      console.error('[SitePublishingStore] Failed to create access token:', error);
      return null;
    } finally {
      this.isCreatingToken = false;
    }
  }

  async refreshTokens(workspaceId?: string): Promise<void> {
    if (this.isRefreshingTokens) return;

    const resolvedWorkspaceId = this.resolveWorkspaceId(workspaceId);
    if (!resolvedWorkspaceId) return;

    if (!this.site) {
      this.tokens = [];
      return;
    }

    this.isRefreshingTokens = true;

    try {
      const tokens = await listTokens(resolvedWorkspaceId);
      this.tokens = [...tokens].sort((a, b) => b.created_at - a.created_at);
    } catch (error) {
      this.error = getErrorMessage(error);
      console.error('[SitePublishingStore] Failed to list access tokens:', error);
    } finally {
      this.isRefreshingTokens = false;
    }
  }

  async revokeToken(tokenId: string, workspaceId?: string): Promise<boolean> {
    if (this.isRevokingToken) return false;

    const resolvedWorkspaceId = this.resolveWorkspaceId(workspaceId);
    if (!resolvedWorkspaceId) return false;

    this.isRevokingToken = true;
    this.error = null;

    try {
      await revokeToken(resolvedWorkspaceId, tokenId);
      this.tokens = this.tokens.filter((token) => token.id !== tokenId);
      return true;
    } catch (error) {
      this.error = getErrorMessage(error);
      console.error('[SitePublishingStore] Failed to revoke token:', error);
      return false;
    } finally {
      this.isRevokingToken = false;
    }
  }

  // Test helper / manual reset
  reset() {
    this.site = null;
    this.audiences = [];
    this.tokens = [];
    this.error = null;
    this.lastPublishedAt = null;
    this.lastCreatedAccessUrl = null;
    this.isLoading = false;
    this.isCreatingSite = false;
    this.isDeletingSite = false;
    this.isPublishing = false;
    this.isCreatingToken = false;
    this.isRevokingToken = false;
    this.isRefreshingTokens = false;
  }
}

// ============================================================================
// Convenience export
// ============================================================================

export const sitePublishingStore = new SitePublishingStore();

export function getSitePublishingStore() {
  return sitePublishingStore;
}
