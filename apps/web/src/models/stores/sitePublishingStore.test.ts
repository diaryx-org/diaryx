import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/auth', () => ({
  getDefaultWorkspace: vi.fn(),
}));

vi.mock('../services/sitePublishingService', () => ({
  getSite: vi.fn(),
  createSite: vi.fn(),
  deleteSite: vi.fn(),
  publishSite: vi.fn(),
  createToken: vi.fn(),
  listTokens: vi.fn(),
  revokeToken: vi.fn(),
}));

import { getDefaultWorkspace } from '$lib/auth';
import {
  createToken,
  getSite,
  listTokens,
  publishSite,
  revokeToken,
} from '../services/sitePublishingService';
import { getSitePublishingStore } from './sitePublishingStore.svelte';

describe('sitePublishingStore', () => {
  const store = getSitePublishingStore();

  beforeEach(() => {
    vi.clearAllMocks();
    store.reset();
    vi.mocked(getDefaultWorkspace).mockReturnValue({ id: 'workspace-1', name: 'default' } as any);
  });

  it('disables publishing actions when default workspace is missing', async () => {
    vi.mocked(getDefaultWorkspace).mockReturnValue(null);

    await store.load();

    expect(store.hasDefaultWorkspace).toBe(false);
    expect(store.canPublish).toBe(false);
    expect(store.error).toBe('No default workspace is available for publishing.');
  });

  it('loads site state and token list when site exists', async () => {
    vi.mocked(getSite).mockResolvedValue({
      site: {
        id: 'site-1',
        workspace_id: 'workspace-1',
        slug: 'my-site',
        enabled: true,
        auto_publish: true,
        last_published_at: 1730000000,
        created_at: 1730000000,
        updated_at: 1730000000,
      },
      audiences: [{ name: 'public', file_count: 12, built_at: 1730000000 }],
    });
    vi.mocked(listTokens).mockResolvedValue([
      {
        id: 'token-1',
        audience: 'public',
        label: null,
        expires_at: null,
        revoked: false,
        created_at: 1730000001,
      },
    ]);

    await store.load();

    expect(store.site?.slug).toBe('my-site');
    expect(store.audiences).toEqual([{ name: 'public', file_count: 12, built_at: 1730000000 }]);
    expect(store.tokens).toHaveLength(1);
  });

  it('clears state when no site is configured', async () => {
    store.site = {
      id: 'site-1',
      workspace_id: 'workspace-1',
      slug: 'my-site',
      enabled: true,
      auto_publish: true,
      last_published_at: null,
      created_at: 1730000000,
      updated_at: 1730000000,
    };
    store.tokens = [{
      id: 'token-1',
      audience: 'public',
      label: null,
      expires_at: null,
      revoked: false,
      created_at: 1730000001,
    }];

    vi.mocked(getSite).mockResolvedValue(null);

    await store.load();

    expect(store.site).toBeNull();
    expect(store.tokens).toEqual([]);
    expect(listTokens).not.toHaveBeenCalled();
  });

  it('prevents duplicate publish requests while one is in progress', async () => {
    store.site = {
      id: 'site-1',
      workspace_id: 'workspace-1',
      slug: 'my-site',
      enabled: true,
      auto_publish: true,
      last_published_at: null,
      created_at: 1730000000,
      updated_at: 1730000000,
    };

    let resolvePublish!: (value: {
      slug: string;
      audiences: { name: string; file_count: number }[];
      published_at: number;
    }) => void;
    const publishPromise = new Promise<{
      slug: string;
      audiences: { name: string; file_count: number }[];
      published_at: number;
    }>((resolve) => {
      resolvePublish = resolve;
    });

    vi.mocked(publishSite).mockReturnValue(publishPromise as Promise<any>);

    const first = store.publishNow();
    const second = await store.publishNow();

    expect(second).toBe(false);
    expect(publishSite).toHaveBeenCalledTimes(1);

    resolvePublish({
      slug: 'my-site',
      audiences: [{ name: 'public', file_count: 15 }],
      published_at: 1730001234,
    });

    await expect(first).resolves.toBe(true);
    expect(store.lastPublishedAt).toBe(1730001234);
  });

  it('stores one-time access URL and prepends created token', async () => {
    store.site = {
      id: 'site-1',
      workspace_id: 'workspace-1',
      slug: 'my-site',
      enabled: true,
      auto_publish: true,
      last_published_at: null,
      created_at: 1730000000,
      updated_at: 1730000000,
    };

    vi.mocked(createToken).mockResolvedValue({
      token: {
        id: 'token-2',
        audience: 'family',
        label: 'Cousins',
        expires_at: 1730000900,
        revoked: false,
        created_at: 1730000002,
      },
      accessUrl: 'https://sites.example.com/my-site?access=abc',
    });

    await store.createToken({ audience: 'family', label: 'Cousins', expires_in: '7d' });

    expect(store.tokens[0].id).toBe('token-2');
    expect(store.lastCreatedAccessUrl).toBe('https://sites.example.com/my-site?access=abc');
  });

  it('removes token from local state after revoke', async () => {
    store.site = {
      id: 'site-1',
      workspace_id: 'workspace-1',
      slug: 'my-site',
      enabled: true,
      auto_publish: true,
      last_published_at: null,
      created_at: 1730000000,
      updated_at: 1730000000,
    };

    store.tokens = [
      {
        id: 'token-1',
        audience: 'public',
        label: null,
        expires_at: null,
        revoked: false,
        created_at: 1730000001,
      },
      {
        id: 'token-2',
        audience: 'family',
        label: 'Cousins',
        expires_at: null,
        revoked: false,
        created_at: 1730000002,
      },
    ];

    vi.mocked(revokeToken).mockResolvedValue(undefined);

    const result = await store.revokeToken('token-1');

    expect(result).toBe(true);
    expect(store.tokens.map((token) => token.id)).toEqual(['token-2']);
  });
});
