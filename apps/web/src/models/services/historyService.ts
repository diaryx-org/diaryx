/**
 * History Service — API client for git-backed version history.
 *
 * Communicates with the sync server's history/commit/restore endpoints.
 */

import { getToken, getServerUrl, getCurrentWorkspace } from '$lib/auth';

// ============================================================================
// Types
// ============================================================================

export interface CommitLogEntry {
  id: string;
  short_id: string;
  message: string;
  timestamp: string;
  file_count: number;
}

export interface CommitResponse {
  commit_id: string;
  file_count: number;
  compacted: boolean;
}

export interface RestoreResponse {
  restored_from: string;
  file_count: number;
}

// ============================================================================
// API Helpers
// ============================================================================

function getApiBase(): { serverUrl: string; token: string; workspaceId: string } | null {
  const serverUrl = getServerUrl();
  const token = getToken();
  const workspace = getCurrentWorkspace();
  if (!serverUrl || !token || !workspace) return null;
  return { serverUrl: serverUrl.replace(/\/$/, ''), token, workspaceId: workspace.id };
}

async function apiFetch<T>(
  path: string,
  options?: RequestInit,
): Promise<T> {
  const base = getApiBase();
  if (!base) throw new Error('Not authenticated or no workspace');

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
    const text = await response.text();
    throw new Error(text || `Request failed: ${response.status}`);
  }

  return response.json();
}

// ============================================================================
// Public API
// ============================================================================

/**
 * Get git commit history for the current workspace.
 */
export async function getCommitHistory(count = 20): Promise<CommitLogEntry[]> {
  return apiFetch<CommitLogEntry[]>(`/history?count=${count}`);
}

/**
 * Trigger an immediate git commit for the current workspace.
 */
export async function commitWorkspace(message?: string): Promise<CommitResponse> {
  return apiFetch<CommitResponse>('/commit', {
    method: 'POST',
    body: JSON.stringify({ message: message ?? null }),
  });
}

/**
 * Restore workspace CRDT state from a specific git commit.
 */
export async function restoreWorkspace(commitId: string): Promise<RestoreResponse> {
  return apiFetch<RestoreResponse>('/restore', {
    method: 'POST',
    body: JSON.stringify({ commit_id: commitId }),
  });
}

/**
 * Check if history API is available (user is authenticated with a workspace).
 */
export function isHistoryAvailable(): boolean {
  return getApiBase() !== null;
}
