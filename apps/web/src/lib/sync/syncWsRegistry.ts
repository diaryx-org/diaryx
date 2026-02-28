import type { Backend } from "../backend/interface";

/**
 * Request envelope sent from the host transport to a registered sync handler.
 */
export type SyncWsRequest =
  | { type: "connected"; serverUrl: string }
  | { type: "disconnected"; reason?: string }
  | { type: "incoming_binary"; data: Uint8Array }
  | { type: "incoming_text"; text: string }
  | { type: "local_update"; docId: string; data: Uint8Array }
  | { type: "focus"; files: string[] }
  | { type: "unfocus"; files: string[] }
  | { type: "request_body"; files: string[] }
  | { type: "snapshot_imported" };

/**
 * Outgoing messages/events produced by a sync handler.
 */
export interface SyncWsDrainResult {
  binary: Uint8Array[];
  text: string[];
  events: string[];
}

/**
 * Plugin-registered sync protocol handler.
 */
export interface SyncWsHandler {
  handle(request: SyncWsRequest): Promise<void>;
  drain(): Promise<SyncWsDrainResult>;
  destroy?(): Promise<void> | void;
}

/**
 * Context provided to handler factories during transport initialization.
 */
export interface SyncWsHandlerFactoryOptions {
  pluginId: string;
  backend: Backend;
  serverUrl: string;
  workspaceId: string;
  writeToDisk: boolean;
  authToken?: string;
  sessionCode?: string;
}

export type SyncWsHandlerFactory = (
  options: SyncWsHandlerFactoryOptions,
) => Promise<SyncWsHandler>;

const handlerFactories = new Map<string, SyncWsHandlerFactory>();

export function registerSyncWsHandler(
  pluginId: string,
  factory: SyncWsHandlerFactory,
): void {
  handlerFactories.set(pluginId, factory);
}

export function unregisterSyncWsHandler(pluginId: string): void {
  handlerFactories.delete(pluginId);
}

export function getSyncWsHandlerFactory(
  pluginId: string,
): SyncWsHandlerFactory | undefined {
  return handlerFactories.get(pluginId);
}
