<script lang="ts">
  import { onMount } from 'svelte';
  import {
    getCommitHistory,
    commitWorkspace,
    isHistoryAvailable,
    type CommitLogEntry,
  } from '@/models/services/historyService';
  import { getBackend, createApi } from '$lib/backend';
  import {
    buildWorkspaceSnapshotUploadBlob,
    findWorkspaceRootPath,
    resolveWorkspaceDir,
  } from '$lib/settings/workspaceSnapshotUpload';
  import {
    uploadWorkspaceSnapshot,
    downloadWorkspaceSnapshot,
    getCurrentWorkspace,
    isAuthenticated as checkAuthenticated,
    isSyncEnabled,
  } from '$lib/auth';

  import type JSZipType from 'jszip';

  // State — history view
  let commits: CommitLogEntry[] = $state([]);
  let loading = $state(true);
  let committing = $state(false);
  let overwriting = $state(false);
  let error = $state<string | null>(null);
  let available = $state(false);

  // State — restore view
  let view: 'history' | 'restore' = $state('history');
  let restoreFiles: { name: string; selected: boolean }[] = $state([]);
  let restoreZip: JSZipType | null = $state(null);
  let restoreLoading = $state(false);
  let restoring = $state(false);

  // Derived
  let selectedCount = $derived(restoreFiles.filter(f => f.selected).length);
  let allSelected = $derived(restoreFiles.length > 0 && restoreFiles.every(f => f.selected));

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

  async function handleOverwrite() {
    if (
      !confirm(
        'This will replace all server data with your local files. Continue?',
      )
    ) {
      return;
    }

    overwriting = true;
    error = null;
    try {
      const workspace = getCurrentWorkspace();
      if (!workspace) throw new Error('No workspace selected');

      const backend = await getBackend();
      const api = createApi(backend);
      const workspaceRootPath = await findWorkspaceRootPath(api, backend);
      if (!workspaceRootPath) throw new Error('Could not find workspace root');

      const snapshot = await buildWorkspaceSnapshotUploadBlob(api, workspaceRootPath);
      await uploadWorkspaceSnapshot(workspace.id, snapshot.blob, 'replace', true);

      alert(`Server overwritten with ${snapshot.filesAdded} local files.`);
    } catch (e) {
      error = e instanceof Error ? e.message : 'Overwrite failed';
      console.error('[GitHistoryPanel] Overwrite error:', e);
    } finally {
      overwriting = false;
    }
  }

  /** Start the file-picker restore flow. commitId=null means latest server state. */
  async function startRestore(commitId: string | null) {
    restoreLoading = true;
    restoreFiles = [];
    restoreZip = null;
    error = null;
    view = 'restore';

    try {
      const workspace = getCurrentWorkspace();
      if (!workspace) throw new Error('No workspace selected');

      const blob = await downloadWorkspaceSnapshot(
        workspace.id,
        true,
        commitId ?? undefined,
      );
      if (!blob) throw new Error('Failed to download snapshot');

      const JSZip = (await import('jszip')).default;
      const zip = await JSZip.loadAsync(blob);

      const files = Object.keys(zip.files)
        .filter(name => !zip.files[name].dir)
        .filter(name => !name.startsWith('.') && !name.startsWith('__MACOSX'))
        .sort();

      restoreFiles = files.map(name => ({ name, selected: true }));
      restoreZip = zip;
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to load snapshot';
      console.error('[GitHistoryPanel] Restore load error:', e);
    } finally {
      restoreLoading = false;
    }
  }

  function cancelRestore() {
    view = 'history';
    restoreFiles = [];
    restoreZip = null;
    error = null;
  }

  function toggleSelectAll() {
    const newValue = !allSelected;
    restoreFiles = restoreFiles.map(f => ({ ...f, selected: newValue }));
  }

  const TEXT_EXTENSIONS = new Set(['.md', '.txt', '.json', '.yaml', '.yml', '.toml', '.csv', '.xml', '.html', '.css', '.js', '.ts', '.svg']);

  function isTextFile(name: string): boolean {
    const dotIdx = name.lastIndexOf('.');
    if (dotIdx === -1) return false;
    return TEXT_EXTENSIONS.has(name.slice(dotIdx).toLowerCase());
  }

  async function restoreSelected() {
    if (!restoreZip || selectedCount === 0) return;

    restoring = true;
    error = null;
    try {
      const backend = await getBackend();
      const api = createApi(backend);
      const workspaceDir = resolveWorkspaceDir(backend);
      let count = 0;

      for (const file of restoreFiles) {
        if (!file.selected) continue;
        const zipEntry = restoreZip.files[file.name];
        if (!zipEntry) continue;

        const localPath = workspaceDir + '/' + file.name;
        if (isTextFile(file.name)) {
          const content = await zipEntry.async('string');
          await api.writeFile(localPath, content);
        } else {
          const data = await zipEntry.async('uint8array');
          await api.writeBinary(localPath, data);
        }
        count++;
      }

      alert(`Restored ${count} file${count !== 1 ? 's' : ''} from server.`);
      cancelRestore();
    } catch (e) {
      error = e instanceof Error ? e.message : 'Restore failed';
      console.error('[GitHistoryPanel] Restore error:', e);
    } finally {
      restoring = false;
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
  {#if view === 'restore'}
    <!-- Restore file picker view -->
    <div class="header">
      <div class="header-actions">
        <button class="cancel-btn" onclick={cancelRestore} disabled={restoring}>
          Cancel
        </button>
        <button
          class="restore-confirm-btn"
          onclick={restoreSelected}
          disabled={restoring || selectedCount === 0}
        >
          {restoring ? 'Restoring...' : `Restore Selected (${selectedCount})`}
        </button>
      </div>
    </div>

    {#if error}
      <div class="error">{error}</div>
    {/if}

    {#if restoreLoading}
      <div class="loading">Downloading snapshot...</div>
    {:else if restoreFiles.length === 0 && !error}
      <div class="empty">No files found in snapshot.</div>
    {:else}
      <div class="select-all-row">
        <label class="select-all-label">
          <input
            type="checkbox"
            checked={allSelected}
            onchange={toggleSelectAll}
          />
          {allSelected ? 'Deselect All' : 'Select All'}
          <span class="file-count-badge">{restoreFiles.length} files</span>
        </label>
      </div>
      <div class="file-list">
        {#each restoreFiles as file, i (file.name)}
          <label class="file-entry">
            <input
              type="checkbox"
              bind:checked={restoreFiles[i].selected}
            />
            <span class="file-name" title={file.name}>{file.name}</span>
            <span class="file-type">{isTextFile(file.name) ? 'text' : 'binary'}</span>
          </label>
        {/each}
      </div>
    {/if}
  {:else}
    <!-- History view -->
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
        <button
          class="overwrite-btn"
          onclick={handleOverwrite}
          disabled={overwriting || !checkAuthenticated() || !isSyncEnabled()}
          title="Replace all server data with local files"
        >
          {overwriting ? 'Overwriting...' : 'Overwrite Server'}
        </button>
        <button
          class="restore-server-btn"
          onclick={() => startRestore(null)}
          disabled={!checkAuthenticated() || !isSyncEnabled()}
          title="Restore files from the latest server state"
        >
          Restore from Server
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
                onclick={() => startRestore(commit.id)}
              >
                Restore
              </button>
            </div>
          </div>
        {/each}
      </div>
    {/if}
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
    flex-wrap: wrap;
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

  .overwrite-btn {
    padding: 0.25rem 0.5rem;
    font-size: 0.8rem;
    background: var(--destructive);
    color: var(--destructive-foreground);
    border: none;
    border-radius: 4px;
    cursor: pointer;
  }

  .overwrite-btn:hover:not(:disabled) {
    opacity: 0.9;
  }

  .overwrite-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .restore-server-btn {
    padding: 0.25rem 0.5rem;
    font-size: 0.8rem;
    background: var(--primary);
    color: var(--primary-foreground);
    border: none;
    border-radius: 4px;
    cursor: pointer;
  }

  .restore-server-btn:hover:not(:disabled) {
    opacity: 0.9;
  }

  .restore-server-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .cancel-btn {
    padding: 0.25rem 0.5rem;
    font-size: 0.8rem;
    background: var(--muted);
    border: 1px solid var(--border);
    border-radius: 4px;
    cursor: pointer;
  }

  .cancel-btn:hover:not(:disabled) {
    background: var(--accent);
  }

  .restore-confirm-btn {
    padding: 0.25rem 0.5rem;
    font-size: 0.8rem;
    background: var(--primary);
    color: var(--primary-foreground);
    border: none;
    border-radius: 4px;
    cursor: pointer;
  }

  .restore-confirm-btn:hover:not(:disabled) {
    opacity: 0.9;
  }

  .restore-confirm-btn:disabled {
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

  /* Restore view styles */

  .select-all-row {
    margin-bottom: 0.5rem;
    padding-bottom: 0.5rem;
    border-bottom: 1px solid var(--border);
  }

  .select-all-label {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.85rem;
    cursor: pointer;
  }

  .file-count-badge {
    font-size: 0.75rem;
    color: var(--muted-foreground);
    margin-left: auto;
  }

  .file-list {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.125rem;
  }

  .file-entry {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.25rem 0.375rem;
    border-radius: 4px;
    cursor: pointer;
    font-size: 0.8rem;
  }

  .file-entry:hover {
    background: var(--accent);
  }

  .file-name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: monospace;
    font-size: 0.75rem;
  }

  .file-type {
    font-size: 0.7rem;
    color: var(--muted-foreground);
    flex-shrink: 0;
  }
</style>
