/**
 * Services Re-exports
 *
 * Central export point for all services.
 */

export {
  revokeBlobUrls,
  transformAttachmentPaths,
  reverseBlobUrlsToAttachmentPaths,
  trackBlobUrl,
  getBlobUrl,
  getPathForBlobUrl,
  hasBlobUrls,
  clearAttachmentThumbnailCache,
  clearAttachmentVerificationCache,
  attachmentExistsLocally,
  getAttachmentAvailability,
  getCachedAttachmentThumbnailUrl,
  getAttachmentThumbnailUrl,
  getAttachmentMediaKind,
  formatMarkdownDestination,
  isPreviewableAttachmentKind,
  resolvePreviewMediaSrc,
  resolvePreviewImageSrc,
  computeRelativeAttachmentPath,
  type AttachmentMediaKind,
} from './attachmentService';

export {
  showError,
  showSuccess,
  showWarning,
  showInfo,
  showLoading,
  handleError,
} from './toastService';

export {
  checkForAppUpdatesInBackground,
  installAvailableAppUpdate,
} from './updaterService';

export {
  getCommitHistory,
  commitWorkspace,
  restoreWorkspace,
  isHistoryAvailable,
  type CommitLogEntry,
  type CommitResponse,
  type RestoreResponse,
} from './historyService';


export {
  setAttachmentSyncBackend,
  setAttachmentSyncContext,
  enqueueAttachmentUpload,
  enqueueAttachmentDownload,
  enqueueMissingDownloadsFromMetadata,
  requestMissingBlobDownload,
  retryFailedAttachmentJobs,
  indexAttachmentRefs,
  sha256Hex,
  isTerminalAttachmentSyncError,
  getAttachmentSyncQueueSnapshot,
} from '$lib/sync/attachmentSyncService';
