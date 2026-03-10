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
  getCommitHistory,
  commitWorkspace,
  restoreWorkspace,
  isHistoryAvailable,
  type CommitLogEntry,
  type CommitResponse,
  type RestoreResponse,
} from './historyService';

export {
  getSite,
  createSite,
  deleteSite,
  publishSite,
  createToken,
  listTokens,
  revokeToken,
  isSitePublishingAvailable,
  type PublishedSite,
  type AudienceBuildSummary,
  type PublishResult,
  type SiteAccessToken,
  type CreateSiteRequest,
  type CreateTokenRequest,
  type ApiError,
} from './sitePublishingService';

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
