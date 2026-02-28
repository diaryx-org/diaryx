/**
 * Sync plugin module — on-demand loading of the Extism sync WASM plugin.
 *
 * ## Usage
 *
 * ```ts
 * import { loadSyncPlugin, getSyncPlugin, isSyncPluginLoaded } from '$lib/sync';
 * ```
 */

export { ExtismSyncPlugin } from './extismSyncPlugin';
export type {
  GuestManifest,
  CommandRequest,
  CommandResponse,
  DecodedAction,
  DrainResult,
} from './extismSyncPlugin';

export { loadSyncPlugin, getSyncPlugin, isSyncPluginLoaded, unloadSyncPlugin } from './loader';
export type { LoadSyncPluginOptions } from './loader';

export { buildHostFunctions } from './hostFunctions';
export type { SyncHostContext } from './hostFunctions';

export { ExtismSyncBackendAdapter, createSyncOverlay } from './extismSyncBackend';
export type { SyncBackendMethods } from './extismSyncBackend';
export { createExtismSyncWsHandlerFactory } from './extismSyncBackend';

export {
  registerSyncWsHandler,
  unregisterSyncWsHandler,
  getSyncWsHandlerFactory,
} from './syncWsRegistry';
export type {
  SyncWsRequest,
  SyncWsDrainResult,
  SyncWsHandler,
  SyncWsHandlerFactory,
  SyncWsHandlerFactoryOptions,
} from './syncWsRegistry';

export {
  SYNC_PLUGIN_ID,
  SYNC_BUILTIN_TABS,
  SYNC_STATUS_ITEM_IDS,
  isSyncPluginId,
  isBuiltinComponentId,
  isSyncBuiltinSidebarTab,
  getSyncBuiltinTabKeyByComponentId,
  isSyncStatusBarItem,
} from './syncBuiltinUiRegistry';
