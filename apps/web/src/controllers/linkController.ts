/**
 * Link Controller
 *
 * Handles link clicks in the editor:
 * - External links (http://, https://) open in new tab
 * - Relative links navigate to other notes
 * - Broken relative links offer to create the file
 */

import type { Api, TreeNode, EntryData } from '../lib/backend';
import { uiStore } from '../models/stores';

/**
 * Handle link clicks in the editor.
 *
 * @param href - The link href from the click event
 * @param api - API instance for file operations
 * @param currentEntry - Currently open entry (for resolving relative paths)
 * @param tree - Workspace tree (for workspace root resolution)
 * @param openEntryFn - Function to open an entry
 * @param refreshTreeFn - Function to refresh the tree
 */
export async function handleLinkClick(
  href: string,
  api: Api,
  currentEntry: EntryData | null,
  tree: TreeNode | null,
  openEntryFn: (path: string) => Promise<void>,
  refreshTreeFn: () => Promise<void>
): Promise<void> {
  if (!href) return;

  // External links - open in new tab
  if (href.startsWith('http://') || href.startsWith('https://')) {
    window.open(href, '_blank', 'noopener,noreferrer');
    return;
  }

  // Relative link - resolve against current file's directory
  if (!currentEntry) return;

  // Get the directory of the current file
  const currentDir = currentEntry.path.substring(
    0,
    currentEntry.path.lastIndexOf('/')
  );

  // Resolve the relative path
  let targetPath: string;
  if (href.startsWith('/')) {
    // Absolute path from workspace root
    const workspaceRoot =
      tree?.path?.substring(0, tree.path.lastIndexOf('/')) || '';
    targetPath = workspaceRoot + href;
  } else {
    // Relative path - resolve against current directory
    const parts = [...currentDir.split('/'), ...href.split('/')];
    const resolved: string[] = [];
    for (const part of parts) {
      if (part === '..') {
        resolved.pop();
      } else if (part !== '.' && part !== '') {
        resolved.push(part);
      }
    }
    targetPath = resolved.join('/');
  }

  // Ensure .md extension
  if (!targetPath.endsWith('.md')) {
    targetPath += '.md';
  }

  // Try to open the entry
  try {
    // Check if file exists by trying to get it
    const entry = await api.getEntry(targetPath);
    if (entry) {
      await openEntryFn(targetPath);
      return;
    }
  } catch {
    // File doesn't exist - offer to create it
    const fileName = targetPath.split('/').pop() || 'note.md';
    const create = window.confirm(
      `"${fileName}" doesn't exist.\n\nWould you like to create it?`
    );
    if (create) {
      try {
        // Create the file with basic frontmatter
        const title = fileName.replace('.md', '').replace(/-/g, ' ');
        await api.createEntry(targetPath, { title });
        await refreshTreeFn();
        await openEntryFn(targetPath);
      } catch (e) {
        console.error('Failed to create entry:', e);
        uiStore.setError(e instanceof Error ? e.message : String(e));
      }
    }
  }
}
