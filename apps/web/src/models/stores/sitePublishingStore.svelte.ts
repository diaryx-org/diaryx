/**
 * Site Publishing Store - Manages published-site state in the Share sidebar.
 */

import {
  createServerWorkspace,
  getAuthState,
} from '$lib/auth';
import { createLocalWorkspaceSnapshot } from '$lib/publish/workspaceSnapshot';
import {
  getCurrentWorkspaceId,
  getLocalWorkspace,
  getServerWorkspaceId,
  isWorkspaceSyncEnabled,
  setPluginMetadata,
  type LocalWorkspace,
} from '$lib/storage/localWorkspaceRegistry.svelte';
import { collaborationStore } from './collaborationStore.svelte';
import {
  createSite,
  createToken,
  deleteSite,
  getSite,
  listTokens,
  publishSite,
  removeCustomDomain,
  revokeToken,
  setCustomDomain,
  type ApiError,
  type AudienceBuildSummary,
  type CreateSiteRequest,
  type CreateTokenRequest,
  type PublishResult,
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
  isSettingDomain = $state(false);
  isRemovingDomain = $state(false);

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
    return this.getDefaultWorkspace()?.id ?? null;
  }

  get defaultWorkspaceName(): string | null {
    return this.getDefaultWorkspace()?.name ?? null;
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

  private getDefaultWorkspace(): LocalWorkspace | null {
    const workspaceId = getAuthState().activeWorkspaceId ?? getCurrentWorkspaceId();
    return workspaceId ? getLocalWorkspace(workspaceId) : null;
  }

  private resolveLocalWorkspace(workspaceId?: string): LocalWorkspace | null {
    const workspace = workspaceId
      ? getLocalWorkspace(workspaceId)
      : this.getDefaultWorkspace();

    if (!workspace) {
      this.error = 'No local workspace is available for publishing.';
      return null;
    }

    return workspace;
  }

  private resolveLinkedWorkspace(workspaceId?: string): { localWorkspace: LocalWorkspace; serverWorkspaceId: string } | null {
    const localWorkspace = this.resolveLocalWorkspace(workspaceId);
    if (!localWorkspace) return null;

    const serverWorkspaceId = getServerWorkspaceId(localWorkspace.id);
    if (!serverWorkspaceId) {
      this.error = 'This workspace has not been linked to the server yet.';
      return null;
    }

    return { localWorkspace, serverWorkspaceId };
  }

  private async ensureLinkedWorkspace(workspaceId?: string): Promise<{ localWorkspace: LocalWorkspace; serverWorkspaceId: string } | null> {
    const localWorkspace = this.resolveLocalWorkspace(workspaceId);
    if (!localWorkspace) return null;

    const existingServerWorkspaceId = getServerWorkspaceId(localWorkspace.id);
    if (existingServerWorkspaceId) {
      return { localWorkspace, serverWorkspaceId: existingServerWorkspaceId };
    }

    const remoteWorkspace = await createServerWorkspace(localWorkspace.name);
    setPluginMetadata(localWorkspace.id, 'sync', {
      serverId: remoteWorkspace.id,
      syncEnabled: false,
    });

    return { localWorkspace, serverWorkspaceId: remoteWorkspace.id };
  }

  private shouldUseSyncFastPath(localWorkspaceId: string): boolean {
    return (
      isWorkspaceSyncEnabled(localWorkspaceId)
      && collaborationStore.collaborationEnabled
      && collaborationStore.effectiveSyncStatus === 'synced'
    );
  }

  async load(workspaceId?: string): Promise<void> {
    if (this.isLoading) return;

    const localWorkspace = this.resolveLocalWorkspace(workspaceId);
    if (!localWorkspace) {
      this.site = null;
      this.audiences = [];
      this.tokens = [];
      this.lastPublishedAt = null;
      return;
    }

    const resolvedWorkspaceId = getServerWorkspaceId(localWorkspace.id);
    if (!resolvedWorkspaceId) {
      this.site = null;
      this.audiences = [];
      this.tokens = [];
      this.lastPublishedAt = null;
      this.lastCreatedAccessUrl = null;
      this.error = null;
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

      await this.refreshTokens(localWorkspace.id);
    } catch (error) {
      this.error = getErrorMessage(error);
      console.error('[SitePublishingStore] Failed to load publishing state:', error);
    } finally {
      this.isLoading = false;
    }
  }

  async create(input: CreateSiteRequest, workspaceId?: string): Promise<PublishedSite | null> {
    if (this.isCreatingSite) return null;

    const workspace = await this.ensureLinkedWorkspace(workspaceId);
    if (!workspace) return null;

    this.isCreatingSite = true;
    this.error = null;

    try {
      const site = await createSite(workspace.serverWorkspaceId, input);
      this.site = site;
      this.audiences = [];
      this.tokens = [];
      this.lastPublishedAt = site.last_published_at;
      this.lastCreatedAccessUrl = null;
      await this.refreshTokens(workspace.localWorkspace.id);
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

    const workspace = this.resolveLinkedWorkspace(workspaceId);
    if (!workspace) return false;

    this.isDeletingSite = true;
    this.error = null;

    try {
      await deleteSite(workspace.serverWorkspaceId);
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

  async publishNow(audience?: string, workspaceId?: string): Promise<boolean> {
    if (this.isPublishing) return false;

    const workspace = await this.ensureLinkedWorkspace(workspaceId);
    if (!workspace) return false;

    this.isPublishing = true;
    this.error = null;

    try {
      const useSyncFastPath = this.shouldUseSyncFastPath(workspace.localWorkspace.id);
      let result: PublishResult;
      if (useSyncFastPath) {
        try {
          result = await publishSite(workspace.serverWorkspaceId, { audience });
        } catch (error) {
          if ((error as ApiError | undefined)?.code !== 'snapshot_required') {
            throw error;
          }

          const snapshot = await createLocalWorkspaceSnapshot(workspace.localWorkspace.id);
          result = await publishSite(workspace.serverWorkspaceId, { audience, snapshot });
        }
      } else {
        const snapshot = await createLocalWorkspaceSnapshot(workspace.localWorkspace.id);
        result = await publishSite(workspace.serverWorkspaceId, { audience, snapshot });
      }

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

    const workspace = this.resolveLinkedWorkspace(workspaceId);
    if (!workspace) return null;

    this.isCreatingToken = true;
    this.error = null;

    try {
      const result = await createToken(workspace.serverWorkspaceId, input);
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

    const workspace = this.resolveLinkedWorkspace(workspaceId);
    if (!workspace) {
      if (!this.site) this.tokens = [];
      return;
    }

    if (!this.site) {
      this.tokens = [];
      return;
    }

    this.isRefreshingTokens = true;

    try {
      const tokens = await listTokens(workspace.serverWorkspaceId);
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

    const workspace = this.resolveLinkedWorkspace(workspaceId);
    if (!workspace) return false;

    this.isRevokingToken = true;
    this.error = null;

    try {
      await revokeToken(workspace.serverWorkspaceId, tokenId);
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

  async setDomain(domain: string, workspaceId?: string): Promise<boolean> {
    if (this.isSettingDomain) return false;

    const workspace = this.resolveLinkedWorkspace(workspaceId);
    if (!workspace) return false;

    this.isSettingDomain = true;
    this.error = null;

    try {
      const updated = await setCustomDomain(workspace.serverWorkspaceId, domain);
      this.site = updated;
      return true;
    } catch (error) {
      this.error = getErrorMessage(error);
      console.error('[SitePublishingStore] Failed to set custom domain:', error);
      return false;
    } finally {
      this.isSettingDomain = false;
    }
  }

  async removeDomain(workspaceId?: string): Promise<boolean> {
    if (this.isRemovingDomain) return false;

    const workspace = this.resolveLinkedWorkspace(workspaceId);
    if (!workspace) return false;

    this.isRemovingDomain = true;
    this.error = null;

    try {
      await removeCustomDomain(workspace.serverWorkspaceId);
      if (this.site) {
        this.site = { ...this.site, custom_domain: null };
      }
      return true;
    } catch (error) {
      this.error = getErrorMessage(error);
      console.error('[SitePublishingStore] Failed to remove custom domain:', error);
      return false;
    } finally {
      this.isRemovingDomain = false;
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
    this.isSettingDomain = false;
    this.isRemovingDomain = false;
  }
}

// ============================================================================
// Convenience export
// ============================================================================

export const sitePublishingStore = new SitePublishingStore();

export function getSitePublishingStore() {
  return sitePublishingStore;
}
