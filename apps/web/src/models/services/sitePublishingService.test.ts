import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/auth', () => ({
  getToken: vi.fn(),
  getServerUrl: vi.fn(),
}));

import { getServerUrl, getToken } from '$lib/auth';
import {
  createSite,
  createToken,
  deleteSite,
  getSite,
  listTokens,
  publishSite,
  revokeToken,
} from './sitePublishingService';

function jsonResponse(status: number, body: unknown): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    headers: {
      get: () => 'application/json',
    },
    json: async () => body,
  } as unknown as Response;
}

describe('sitePublishingService', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.stubGlobal('fetch', vi.fn());

    vi.mocked(getServerUrl).mockReturnValue('https://sync.example.com/');
    vi.mocked(getToken).mockReturnValue('test-token');
  });

  it('returns null from getSite when backend returns 404', async () => {
    vi.mocked(fetch).mockResolvedValueOnce(
      jsonResponse(404, { error: 'not_found', message: 'Site not found' }),
    );

    const result = await getSite('workspace-1');
    expect(result).toBeNull();
  });

  it('fetches site and audience summary', async () => {
    vi.mocked(fetch).mockResolvedValueOnce(
      jsonResponse(200, {
        id: 'site-1',
        workspace_id: 'workspace-1',
        slug: 'my-site',
        enabled: true,
        auto_publish: true,
        last_published_at: 1730000000,
        created_at: 1730000000,
        updated_at: 1730000000,
        audiences: [{ name: 'public', file_count: 12, built_at: 1730000000 }],
      }),
    );

    const result = await getSite('workspace-1');

    expect(result?.site.slug).toBe('my-site');
    expect(result?.audiences).toEqual([{ name: 'public', file_count: 12, built_at: 1730000000 }]);
  });

  it('posts create site payload with auth headers', async () => {
    vi.mocked(fetch).mockResolvedValueOnce(
      jsonResponse(201, {
        id: 'site-1',
        workspace_id: 'workspace-1',
        slug: 'my-site',
        enabled: true,
        auto_publish: true,
        last_published_at: null,
        created_at: 1730000000,
        updated_at: 1730000000,
        audiences: [],
      }),
    );

    await createSite('workspace-1', {
      slug: 'my-site',
      enabled: true,
      auto_publish: true,
    });

    expect(fetch).toHaveBeenCalledWith(
      'https://sync.example.com/api/workspaces/workspace-1/site',
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({
          Authorization: 'Bearer test-token',
          'Content-Type': 'application/json',
        }),
      }),
    );
  });

  it('maps publish conflict error to deterministic message', async () => {
    vi.mocked(fetch).mockResolvedValueOnce(
      jsonResponse(409, {
        error: 'publish_in_progress',
        message: 'Site publish is currently running',
      }),
    );

    await expect(publishSite('workspace-1')).rejects.toMatchObject({
      status: 409,
      code: 'publish_in_progress',
      message: 'A publish is already in progress for this workspace.',
    });
  });

  it('creates token and returns one-time access URL', async () => {
    vi.mocked(fetch).mockResolvedValueOnce(
      jsonResponse(201, {
        id: 'token-1',
        audience: 'family',
        label: 'Cousins',
        expires_at: 1730000900,
        created_at: 1730000000,
        access_url: 'https://sites.example.com/my-site?access=abc',
      }),
    );

    const result = await createToken('workspace-1', {
      audience: 'family',
      label: 'Cousins',
      expires_in: '7d',
    });

    expect(result.accessUrl).toBe('https://sites.example.com/my-site?access=abc');
    expect(result.token).toMatchObject({
      id: 'token-1',
      audience: 'family',
      label: 'Cousins',
      revoked: false,
    });
  });

  it('lists tokens without secrets', async () => {
    vi.mocked(fetch).mockResolvedValueOnce(
      jsonResponse(200, [
        {
          id: 'token-1',
          audience: 'public',
          label: null,
          expires_at: null,
          revoked: false,
          created_at: 1730000000,
        },
      ]),
    );

    const result = await listTokens('workspace-1');

    expect(result).toEqual([
      {
        id: 'token-1',
        audience: 'public',
        label: null,
        expires_at: null,
        revoked: false,
        created_at: 1730000000,
      },
    ]);
  });

  it('issues delete calls for site and token revoke', async () => {
    vi.mocked(fetch)
      .mockResolvedValueOnce(jsonResponse(204, null))
      .mockResolvedValueOnce(jsonResponse(204, null));

    await deleteSite('workspace-1');
    await revokeToken('workspace-1', 'token-1');

    expect(fetch).toHaveBeenNthCalledWith(
      1,
      'https://sync.example.com/api/workspaces/workspace-1/site',
      expect.objectContaining({ method: 'DELETE' }),
    );
    expect(fetch).toHaveBeenNthCalledWith(
      2,
      'https://sync.example.com/api/workspaces/workspace-1/site/tokens/token-1',
      expect.objectContaining({ method: 'DELETE' }),
    );
  });

  it('fails early when auth context is missing', async () => {
    vi.mocked(getToken).mockReturnValueOnce(null);

    await expect(createSite('workspace-1', { slug: 'my-site' })).rejects.toMatchObject({
      status: 401,
      code: 'not_authenticated',
      message: 'Not authenticated or missing workspace.',
    });
  });
});
