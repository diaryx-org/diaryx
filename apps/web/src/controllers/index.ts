/**
 * Controllers Re-exports
 *
 * Central export point for all controllers.
 * Controllers handle business logic and coordinate between stores, services, and APIs.
 */

export {
  refreshTree,
  loadNodeChildren,
  runValidation,
  validatePath,
  setupWorkspaceCrdt,
} from './workspaceController';

export {
  openEntry,
  saveEntry,
  saveEntryWithSync,
  createChildEntry,
  createChildEntryWithSync,
  createEntry,
  createEntryWithSync,
  ensureDailyEntry,
  deleteEntry,
  deleteEntryWithSync,
  moveEntry,
  handlePropertyChange,
  removeProperty,
  addProperty,
  renameEntry,
  duplicateEntry,
} from './entryController';

export {
  handleValidateWorkspace,
  handleRefreshTree,
  handleDuplicateCurrentEntry,
  handleRenameCurrentEntry,
  handleDeleteCurrentEntry,
  handleMoveCurrentEntry,
  handleCreateChildUnderCurrent,
  handleStartShareSession,
  handleJoinShareSession,
  handleFindInFile,
  handleWordCount,
  handleImportFromClipboard,
  handleCopyAsMarkdown,
  handleViewMarkdown,
} from './commandPaletteController';

export {
  handleAddAttachment,
  handleAttachmentFileSelect,
  handleEditorFileDrop,
  handleDeleteAttachment,
  handleAttachmentInsert,
  handleMoveAttachment,
  populateCrdtBeforeHost,
  fileToBase64,
  getPendingAttachmentPath,
  setPendingAttachmentPath,
  getAttachmentError,
  setAttachmentError,
  clearAttachmentError,
} from './attachmentController';

export { handleLinkClick } from './linkController';
