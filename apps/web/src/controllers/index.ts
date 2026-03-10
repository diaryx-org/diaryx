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
} from './workspaceController';

export {
  openEntry,
  getEditorBodyMarkdown,
  saveEntry,
  saveEntryWithSync,
  createChildEntry,
  createChildEntryWithSync,
  createEntry,
  createEntryWithSync,
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
  handleFindInFile,
  handleWordCount,
  handleImportFromClipboard,
  handleImportMarkdownFile,
  handleCopyAsMarkdown,
  handleViewMarkdown,
  handleReorderFootnotes,
} from './commandPaletteController';

export {
  handleAddAttachment,
  handleAttachmentFileSelect,
  handleEditorFileDrop,
  handleDeleteAttachment,
  handleAttachmentInsert,
  handleMoveAttachment,
  getPendingAttachmentPath,
  setPendingAttachmentPath,
  getAttachmentError,
  setAttachmentError,
  clearAttachmentError,
} from './attachmentController';

export { handleLinkClick } from './linkController';
