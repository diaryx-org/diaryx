import { createApi, getBackend } from '$lib/backend';
import {
  getLocalWorkspace,
  getWorkspaceStoragePluginId,
  getWorkspaceStorageType,
} from '$lib/storage/localWorkspaceRegistry.svelte';
import { addFilesToZip } from '$lib/settings/zipUtils';
import JSZip from 'jszip';

function getWorkspaceDirectoryPath(workspacePath: string): string {
  return workspacePath
    .replace(/\/index\.md$/i, '')
    .replace(/\/README\.md$/i, '');
}

export async function createLocalWorkspaceSnapshot(localWorkspaceId: string): Promise<Blob> {
  const workspace = getLocalWorkspace(localWorkspaceId);
  if (!workspace) {
    throw new Error('No local workspace is available for publishing.');
  }

  const backend = await getBackend(
    workspace.id,
    workspace.name,
    getWorkspaceStorageType(workspace.id),
    getWorkspaceStoragePluginId(workspace.id),
  );
  const api = createApi(backend);
  const workspaceDir = getWorkspaceDirectoryPath(backend.getWorkspacePath());
  const tree = await api.getFilesystemTree(workspaceDir, false);

  const zip = new JSZip();
  const reader = {
    readText: (path: string) => api.readFile(path),
    readBinary: (path: string) => api.readBinary(path),
  };

  const fileCount = await addFilesToZip(zip, tree, workspaceDir, reader);
  if (fileCount === 0) {
    throw new Error('This workspace has no local files to publish yet.');
  }

  return zip.generateAsync({ type: 'blob' });
}
