/**
 * Command Palette Controller
 *
 * Handles command palette actions including:
 * - Workspace validation with feedback
 * - Tree refresh
 * - Entry operations (duplicate, rename, delete, move)
 * - Share session management
 * - Clipboard operations
 * - Word count and find
 */

import { tick } from 'svelte';
import type { TreeNode, Api, EntryData } from '../lib/backend';
import { workspaceStore, shareSessionStore } from '../models/stores';
import { reverseBlobUrlsToAttachmentPaths, joinShareSession } from '../models/services';
import { toast } from 'svelte-sonner';

/**
 * Validate workspace and show toast feedback.
 */
export async function handleValidateWorkspace(
  api: Api,
  tree: TreeNode | null,
  backend: any
): Promise<void> {
  try {
    const rootPath = tree?.path ?? backend.getWorkspacePath();
    const result = await api.validateWorkspace(rootPath);
    workspaceStore.setValidationResult(result);

    const errorCount = result.errors?.length ?? 0;
    const warningCount = result.warnings?.length ?? 0;
    if (errorCount === 0 && warningCount === 0) {
      toast.success('Workspace is valid', { description: 'No issues found' });
    } else {
      toast.warning('Validation complete', {
        description: `${errorCount} error(s), ${warningCount} warning(s) found`,
      });
    }
  } catch (e) {
    toast.error('Validation failed', {
      description: e instanceof Error ? e.message : String(e),
    });
  }
}

/**
 * Refresh tree with toast feedback.
 */
export async function handleRefreshTree(
  refreshTreeFn: () => Promise<void>
): Promise<void> {
  await refreshTreeFn();
  toast.success('Tree refreshed');
}

/**
 * Duplicate the current entry.
 */
export async function handleDuplicateCurrentEntry(
  _api: Api,
  currentEntry: EntryData | null,
  duplicateEntryFn: (path: string) => Promise<string>,
  openEntryFn: (path: string) => Promise<void>
): Promise<void> {
  if (!currentEntry) {
    toast.error('No entry selected');
    return;
  }
  try {
    const newPath = await duplicateEntryFn(currentEntry.path);
    await openEntryFn(newPath);
    toast.success('Entry duplicated', { description: newPath.split('/').pop() });
  } catch (e) {
    toast.error('Failed to duplicate', {
      description: e instanceof Error ? e.message : String(e),
    });
  }
}

/**
 * Rename the current entry (prompt for new name).
 */
export async function handleRenameCurrentEntry(
  _api: Api,
  currentEntry: EntryData | null,
  renameEntryFn: (path: string, newFilename: string) => Promise<string>,
  openEntryFn: (path: string) => Promise<void>
): Promise<void> {
  if (!currentEntry) {
    toast.error('No entry selected');
    return;
  }
  const currentName = currentEntry.path.split('/').pop()?.replace('.md', '') || '';
  const newName = window.prompt('Enter new name:', currentName);
  if (!newName || newName === currentName) return;

  try {
    const newPath = await renameEntryFn(currentEntry.path, newName + '.md');
    await openEntryFn(newPath);
    toast.success('Entry renamed', { description: newName });
  } catch (e) {
    toast.error('Failed to rename', {
      description: e instanceof Error ? e.message : String(e),
    });
  }
}

/**
 * Delete the current entry.
 */
export async function handleDeleteCurrentEntry(
  currentEntry: EntryData | null,
  deleteEntryFn: (path: string) => Promise<void>
): Promise<void> {
  if (!currentEntry) {
    toast.error('No entry selected');
    return;
  }
  await deleteEntryFn(currentEntry.path);
}

/**
 * Move the current entry (prompt for new parent).
 */
export async function handleMoveCurrentEntry(
  _api: Api,
  currentEntry: EntryData | null,
  tree: TreeNode | null,
  moveEntryFn: (entryPath: string, newParentPath: string) => Promise<void>
): Promise<void> {
  if (!currentEntry || !tree) {
    toast.error('No entry selected');
    return;
  }

  // Collect all potential parent paths
  const allEntries: string[] = [];
  function collectPaths(node: TreeNode) {
    if (!node) return;
    // Only index files (with children) can be parents
    if (
      node.children.length > 0 ||
      node.path.endsWith('index.md') ||
      node.path.endsWith('README.md')
    ) {
      allEntries.push(node.path);
    }
    node.children.forEach(collectPaths);
  }
  collectPaths(tree);

  const parentOptions = allEntries
    .filter((p) => p !== currentEntry.path)
    .map((p) => p.split('/').pop()?.replace('.md', '') || p)
    .join(', ');

  const newParentName = window.prompt(
    `Move "${currentEntry.path.split('/').pop()?.replace('.md', '')}" to which parent?\n\nAvailable: ${parentOptions}`
  );
  if (!newParentName) return;

  // Find the matching parent path
  const newParentPath = allEntries.find(
    (p) =>
      p.split('/').pop()?.replace('.md', '').toLowerCase() ===
      newParentName.toLowerCase()
  );
  if (!newParentPath) {
    toast.error('Parent not found', {
      description: `"${newParentName}" is not a valid parent`,
    });
    return;
  }

  try {
    await moveEntryFn(currentEntry.path, newParentPath);
    toast.success('Entry moved', { description: `Moved to ${newParentName}` });
  } catch (e) {
    toast.error('Failed to move', {
      description: e instanceof Error ? e.message : String(e),
    });
  }
}

/**
 * Create a child entry under the current entry.
 */
export async function handleCreateChildUnderCurrent(
  currentEntry: EntryData | null,
  createChildEntryFn: (parentPath: string) => Promise<void>
): Promise<void> {
  if (!currentEntry) {
    toast.error('No entry selected');
    return;
  }
  await createChildEntryFn(currentEntry.path);
  toast.success('Child entry created');
}

/**
 * Start a share session.
 * Opens right sidebar and triggers session start.
 */
export async function handleStartShareSession(
  setRightSidebarCollapsed: (collapsed: boolean) => void,
  setRequestedTab: (tab: 'properties' | 'history' | 'share' | null) => void,
  setTriggerStartSession: (trigger: boolean) => void
): Promise<void> {
  if (shareSessionStore.mode !== 'idle') {
    toast.info('Session already active', { description: 'End current session first' });
    return;
  }
  // Open right sidebar, navigate to share tab, and trigger session start
  setRightSidebarCollapsed(false);
  setRequestedTab('share');
  // Wait for sidebar to render before triggering session start
  await tick();
  setTriggerStartSession(true);
}

/**
 * Join a share session (prompt for code).
 */
export async function handleJoinShareSession(): Promise<void> {
  if (shareSessionStore.mode !== 'idle') {
    toast.info('Session already active', { description: 'End current session first' });
    return;
  }
  const joinCode = window.prompt('Enter join code:');
  if (!joinCode?.trim()) return;

  try {
    workspaceStore.saveTreeState();
    await joinShareSession(joinCode.trim());
    toast.success('Joined session', { description: `Code: ${joinCode.trim()}` });
  } catch (e) {
    workspaceStore.clearSavedTreeState();
    toast.error('Failed to join', {
      description: e instanceof Error ? e.message : String(e),
    });
  }
}

/**
 * Trigger browser's find functionality.
 */
export function handleFindInFile(): void {
  if (typeof window !== 'undefined') {
    try {
      // @ts-ignore - execCommand is deprecated but still works
      document.execCommand('find');
    } catch {
      // Fallback: show keyboard shortcut hint
      toast.info('Find in File', { description: 'Use Cmd/Ctrl+F to search' });
    }
  }
}

/**
 * Show word count for current entry.
 */
export function handleWordCount(
  editorRef: any,
  currentEntry: EntryData | null
): void {
  if (!editorRef || !currentEntry) {
    toast.error('No entry open');
    return;
  }
  const markdown = editorRef.getMarkdown() || '';
  const text = markdown.replace(/[#*_`~\[\]()>-]/g, ' '); // Remove markdown syntax
  const words = text
    .trim()
    .split(/\s+/)
    .filter((w: string) => w.length > 0).length;
  const characters = text.length;
  const lines = markdown.split('\n').length;

  toast.info('Word Count', {
    description: `${words.toLocaleString()} words · ${characters.toLocaleString()} characters · ${lines} lines`,
    duration: 5000,
  });
}

/**
 * Import content from clipboard.
 */
export async function handleImportFromClipboard(
  api: Api,
  tree: TreeNode | null,
  refreshTreeFn: () => Promise<void>,
  openEntryFn: (path: string) => Promise<void>
): Promise<void> {
  if (!tree) {
    toast.error('Workspace not ready');
    return;
  }
  try {
    const clipboardText = await navigator.clipboard.readText();
    if (!clipboardText.trim()) {
      toast.error('Clipboard is empty');
      return;
    }

    // Create a new entry with clipboard content
    const timestamp = new Date()
      .toISOString()
      .replace(/[:.]/g, '-')
      .slice(0, 19);
    const newPath = `${tree.path.replace(/[^/]+\.md$/, '')}imported-${timestamp}.md`;

    // Check if it has frontmatter, if not add a basic title
    let content = clipboardText;
    if (!clipboardText.trim().startsWith('---')) {
      const title = `Imported ${new Date().toLocaleDateString()}`;
      content = `---\ntitle: "${title}"\n---\n\n${clipboardText}`;
    }

    await api.createEntry(newPath, { title: `Imported ${timestamp}` });
    await api.saveEntry(newPath, content);
    await refreshTreeFn();
    await openEntryFn(newPath);
    toast.success('Imported from clipboard', {
      description: `${clipboardText.length} characters`,
    });
  } catch (e) {
    toast.error('Failed to import', {
      description: e instanceof Error ? e.message : String(e),
    });
  }
}

/**
 * Copy current entry content as markdown.
 */
export async function handleCopyAsMarkdown(
  editorRef: any,
  currentEntry: EntryData | null
): Promise<void> {
  if (!editorRef || !currentEntry) {
    toast.error('No entry open');
    return;
  }
  try {
    const markdown = editorRef.getMarkdown() || '';
    // Reverse blob URLs to attachment paths for clean export
    const cleanMarkdown = reverseBlobUrlsToAttachmentPaths(markdown);
    await navigator.clipboard.writeText(cleanMarkdown);
    toast.success('Copied to clipboard', {
      description: `${cleanMarkdown.length} characters`,
    });
  } catch (e) {
    toast.error('Failed to copy', {
      description: e instanceof Error ? e.message : String(e),
    });
  }
}
