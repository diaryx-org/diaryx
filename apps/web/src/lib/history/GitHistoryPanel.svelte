<script lang="ts">
  import { onMount } from 'svelte';
  import {
    getCommitHistory,
    commitWorkspace,
    restoreWorkspace,
    isHistoryAvailable,
    type CommitLogEntry,
  } from '@/models/services/historyService';

  // State
  let commits: CommitLogEntry[] = $state([]);
  let loading = $state(true);
  let committing = $state(false);
  let error = $state<string | null>(null);
  let available = $state(false);

  async function loadHistory() {
    loading = true;
    error = null;
    try {
      commits = await getCommitHistory(50);
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to load history';
      console.error('[GitHistoryPanel] Error loading history:', e);
    } finally {
      loading = false;
    }
  }

  async function handleCommit() {
    committing = true;
    error = null;
    try {
      const result = await commitWorkspace();
      console.log('[GitHistoryPanel] Committed:', result);
      await loadHistory();
    } catch (e) {
      error = e instanceof Error ? e.message : 'Commit failed';
      console.error('[GitHistoryPanel] Commit error:', e);
    } finally {
      committing = false;
    }
  }

  async function handleRestore(commit: CommitLogEntry) {
    if (
      !confirm(
        `Restore workspace to commit ${commit.short_id}?\n\n"${commit.message}"\n\nThis will replace the current CRDT state. Other connected clients should disconnect first.`,
      )
    ) {
      return;
    }

    error = null;
    try {
      const result = await restoreWorkspace(commit.id);
      console.log('[GitHistoryPanel] Restored:', result);
      alert(`Restored ${result.file_count} files from commit ${commit.short_id}`);
    } catch (e) {
      error = e instanceof Error ? e.message : 'Restore failed';
      console.error('[GitHistoryPanel] Restore error:', e);
    }
  }

  function formatTimestamp(iso: string): string {
    const date = new Date(iso);
    return date.toLocaleString();
  }

  function formatRelativeTime(iso: string): string {
    const now = Date.now();
    const diff = now - new Date(iso).getTime();
    const minutes = Math.floor(diff / 60000);
    const hours = Math.floor(diff / 3600000);
    const days = Math.floor(diff / 86400000);

    if (minutes < 1) return 'Just now';
    if (minutes < 60) return `${minutes}m ago`;
    if (hours < 24) return `${hours}h ago`;
    return `${days}d ago`;
  }

  onMount(() => {
    available = isHistoryAvailable();
    if (available) {
      loadHistory();
    } else {
      loading = false;
    }
  });
</script>

<div class="git-history-panel">
  <div class="header">
    <div class="header-actions">
      <button
        class="commit-btn"
        onclick={handleCommit}
        disabled={committing || !available}
      >
        {committing ? 'Committing...' : 'Commit Now'}
      </button>
      <button class="refresh-btn" onclick={loadHistory} disabled={loading || !available}>
        {loading ? 'Loading...' : 'Refresh'}
      </button>
    </div>
  </div>

  {#if !available}
    <div class="empty">
      Sign in and enable sync to use version snapshots.
    </div>
  {:else if error}
    <div class="error">{error}</div>
  {/if}

  {#if loading && commits.length === 0 && available}
    <div class="loading">Loading snapshots...</div>
  {:else if commits.length === 0 && available && !loading}
    <div class="empty">No snapshots yet. Click "Commit Now" to create one.</div>
  {:else}
    <div class="commit-list">
      {#each commits as commit (commit.id)}
        <div class="commit-entry">
          <div class="commit-header">
            <span class="commit-id">{commit.short_id}</span>
            <span class="commit-time" title={formatTimestamp(commit.timestamp)}>
              {formatRelativeTime(commit.timestamp)}
            </span>
          </div>
          <div class="commit-message">{commit.message}</div>
          <div class="commit-footer">
            <span class="commit-files">{commit.file_count} files</span>
            <button
              class="restore-btn"
              onclick={() => handleRestore(commit)}
            >
              Restore
            </button>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .git-history-panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    padding: 1rem;
    overflow: hidden;
  }

  .header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 1rem;
    padding-bottom: 0.5rem;
    border-bottom: 1px solid var(--border);
  }

  .header-actions {
    display: flex;
    gap: 0.25rem;
  }

  .commit-btn {
    padding: 0.25rem 0.5rem;
    font-size: 0.8rem;
    background: var(--primary);
    color: var(--primary-foreground);
    border: none;
    border-radius: 4px;
    cursor: pointer;
  }

  .commit-btn:hover:not(:disabled) {
    opacity: 0.9;
  }

  .commit-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .refresh-btn {
    padding: 0.25rem 0.5rem;
    font-size: 0.8rem;
    background: var(--muted);
    border: 1px solid var(--border);
    border-radius: 4px;
    cursor: pointer;
  }

  .refresh-btn:hover:not(:disabled) {
    background: var(--accent);
  }

  .refresh-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .error {
    padding: 0.5rem;
    color: var(--destructive);
    background: var(--destructive-foreground);
    border-radius: 4px;
    margin-bottom: 1rem;
    font-size: 0.85rem;
  }

  .loading,
  .empty {
    color: var(--muted-foreground);
    text-align: center;
    padding: 2rem;
    font-size: 0.9rem;
  }

  .commit-list {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .commit-entry {
    padding: 0.5rem 0.75rem;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--card);
  }

  .commit-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 0.25rem;
  }

  .commit-id {
    font-family: monospace;
    font-size: 0.8rem;
    color: var(--primary);
    font-weight: 600;
  }

  .commit-time {
    font-size: 0.75rem;
    color: var(--muted-foreground);
  }

  .commit-message {
    font-size: 0.85rem;
    color: var(--foreground);
    margin-bottom: 0.25rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .commit-footer {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .commit-files {
    font-size: 0.75rem;
    color: var(--muted-foreground);
  }

  .restore-btn {
    padding: 0.125rem 0.375rem;
    font-size: 0.75rem;
    background: transparent;
    color: var(--muted-foreground);
    border: 1px solid var(--border);
    border-radius: 3px;
    cursor: pointer;
  }

  .restore-btn:hover {
    background: var(--accent);
    color: var(--foreground);
  }
</style>
