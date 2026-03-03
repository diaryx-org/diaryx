/**
 * Storage module for Diaryx web app.
 *
 * This module provides workspace registry and storage type utilities.
 */

export {
  getLocalWorkspaces,
  getLocalWorkspace,
  getCurrentWorkspaceId,
  isWorkspaceLocal,
  addLocalWorkspace,
  removeLocalWorkspace,
  setCurrentWorkspaceId,
  clearCurrentWorkspaceId,
  renameLocalWorkspace,
  type LocalWorkspace,
} from "./localWorkspaceRegistry.svelte.js";
