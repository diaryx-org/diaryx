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
 * - View markdown source
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
  renameEntryFn: (path: string, newTitle: string) => Promise<string>,
  openEntryFn: (path: string) => Promise<void>
): Promise<void> {
  if (!currentEntry) {
    toast.error('No entry selected');
    return;
  }
  const currentTitle = (typeof currentEntry.frontmatter?.title === 'string' ? currentEntry.frontmatter.title : null)
    || currentEntry.path.split('/').pop()?.replace('.md', '')
    || '';
  const newTitle = window.prompt('Enter new title:', currentTitle);
  if (!newTitle || newTitle === currentTitle) return;

  try {
    const newPath = await renameEntryFn(currentEntry.path, newTitle);
    await openEntryFn(newPath);
    toast.success('Entry renamed', { description: newTitle });
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
 * Opens left sidebar and triggers session start.
 */
export async function handleStartShareSession(
  setLeftSidebarCollapsed: (collapsed: boolean) => void,
  setRequestedTab: (tab: string | null) => void,
  setTriggerStartSession: (trigger: boolean) => void
): Promise<void> {
  if (shareSessionStore.mode !== 'idle') {
    toast.info('Session already active', { description: 'End current session first' });
    return;
  }
  // Open left sidebar, navigate to share tab, and trigger session start
  setLeftSidebarCollapsed(false);
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

    await api.createEntry(newPath, { title: `Imported ${timestamp}`, rootIndexPath: tree?.path });
    await api.saveEntry(newPath, content, tree?.path);
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
 * Import markdown files from a file picker.
 * Files are written to the workspace and attached to the current parent.
 */
export async function handleImportMarkdownFile(
  api: Api,
  tree: TreeNode | null,
  currentEntryPath: string | null,
  refreshTreeFn: () => Promise<void>,
  openEntryFn: (path: string) => Promise<void>
): Promise<void> {
  if (!tree) {
    toast.error('Workspace not ready');
    return;
  }

  // Open file picker for .md files
  const input = document.createElement('input');
  input.type = 'file';
  input.accept = '.md';
  input.multiple = true;

  const files = await new Promise<FileList | null>((resolve) => {
    input.addEventListener('change', () => resolve(input.files));
    // If the user cancels, the change event won't fire. Use a focus fallback.
    window.addEventListener('focus', () => setTimeout(() => resolve(null), 300), { once: true });
    input.click();
  });

  if (!files || files.length === 0) return;

  try {
    // Determine the parent directory for the imported files
    const parentDir = currentEntryPath
      ? currentEntryPath.replace(/[^/]+\.md$/, '')
      : tree.path.replace(/[^/]+\.md$/, '');

    let lastPath = '';
    for (const file of Array.from(files)) {
      const content = await file.text();
      const filename = file.name.replace(/[^\w.-]/g, '-');
      const newPath = `${parentDir}${filename}`;

      // Write the file content
      await api.saveEntry(newPath, content, tree.path);

      // Attach to parent if we have a current entry that's an index
      if (currentEntryPath) {
        try {
          await api.attachEntryToParent(newPath, currentEntryPath);
        } catch {
          // Parent might not be an index — that's ok, file is still written
        }
      }

      lastPath = newPath;
    }

    await refreshTreeFn();
    if (lastPath) {
      await openEntryFn(lastPath);
    }

    toast.success(`Imported ${files.length} file${files.length > 1 ? 's' : ''}`, {
      description: files.length === 1 ? files[0].name : `${files.length} markdown files`,
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

/**
 * Reorder footnotes sequentially based on their position in the document.
 */
export function handleReorderFootnotes(editorRef: any): void {
  if (!editorRef) {
    toast.error('No entry open');
    return;
  }
  editorRef.reorderFootnotes();
  toast.success('Footnotes reordered');
}

/**
 * View the current entry's markdown source.
 * Returns the body markdown and frontmatter so the caller can display them in a dialog.
 */
export function handleViewMarkdown(
  editorRef: any,
  currentEntry: EntryData | null
): { body: string; frontmatter: Record<string, unknown> } | null {
  if (!editorRef || !currentEntry) {
    toast.error('No entry open');
    return null;
  }
  const markdown = editorRef.getMarkdown() || '';
  const body = reverseBlobUrlsToAttachmentPaths(markdown);
  const frontmatter = currentEntry.frontmatter ?? {};
  return { body, frontmatter };
}
