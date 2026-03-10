/**
 * Permission Store - Manages plugin permission requests and session-level cache.
 *
 * Handles the flow: plugin requests permission → check config → check session cache
 * → show banner → user responds → update config or cache.
 */

// ============================================================================
// Types
// ============================================================================

export type PermissionType =
  | 'read_files'
  | 'edit_files'
  | 'create_files'
  | 'delete_files'
  | 'move_files'
  | 'http_requests'
  | 'execute_commands'
  | 'plugin_storage';

export interface PermissionRequest {
  /** Unique ID for this request. */
  id: string;
  /** Which plugin is requesting. */
  pluginId: string;
  /** Human-readable plugin name (for display). */
  pluginName: string;
  /** What kind of permission. */
  permissionType: PermissionType;
  /** The target (file path, URL, command name). */
  target: string;
  /** Resolve callback — called with true (allowed) or false (denied). */
  resolve: (allowed: boolean) => void;
}

export interface PermissionRule {
  include: string[];
  exclude: string[];
}

export interface PluginPermissions {
  read_files?: PermissionRule;
  edit_files?: PermissionRule;
  create_files?: PermissionRule;
  delete_files?: PermissionRule;
  move_files?: PermissionRule;
  http_requests?: PermissionRule;
  execute_commands?: PermissionRule;
  plugin_storage?: PermissionRule;
}

export interface PluginConfig {
  download?: string;
  permissions: PluginPermissions;
}

export interface PermissionPersistenceHandlers {
  /** Return current plugins config from root frontmatter. */
  getPluginsConfig: () => Record<string, PluginConfig> | undefined;
  /** Persist updated plugins config to root frontmatter. */
  savePluginsConfig: (
    config: Record<string, PluginConfig>,
  ) => Promise<void> | void;
}

// ============================================================================
// State
// ============================================================================

/** Queue of pending permission requests (displayed as banners). */
let pendingRequests = $state<PermissionRequest[]>([]);

/** Session-level cache of ephemeral decisions: (pluginId:permType:target) → allowed. */
let sessionCache = $state<Record<string, boolean>>({});

let requestCounter = 0;

let persistenceHandlers = $state<PermissionPersistenceHandlers | null>(null);

/** When true, all permission requests are auto-allowed (for E2E testing). */
let autoAllowAll = false;

// ============================================================================
// Permission Check Logic
// ============================================================================

const PERMISSION_LABELS: Record<PermissionType, string> = {
  read_files: 'read',
  edit_files: 'edit',
  create_files: 'create',
  delete_files: 'delete',
  move_files: 'move',
  http_requests: 'make HTTP requests to',
  execute_commands: 'execute',
  plugin_storage: 'use plugin storage',
};

function cacheKey(pluginId: string, permType: PermissionType, target: string): string {
  return `${pluginId}:${permType}:${target}`;
}

/**
 * Extract domain from a URL.
 */
function extractDomain(url: string): string {
  try {
    const u = new URL(url);
    return u.hostname;
  } catch {
    // Not a valid URL — return as-is
    return url.split('/')[0]?.split(':')[0] ?? url;
  }
}

/**
 * Extract the path from a scope value (markdown link or plain path).
 */
function extractPathFromScope(scope: string): string | null {
  // Markdown link: [Title](path)
  const linkMatch = scope.match(/\]\(([^)]+)\)/);
  if (linkMatch) {
    const path = linkMatch[1];
    return path.startsWith('/') ? path.slice(1) : path;
  }
  // Plain path
  const path = scope.startsWith('/') ? scope.slice(1) : scope;
  return path || null;
}

/**
 * Check if a file path matches a scope pattern.
 */
function pathMatches(filePath: string, patternPath: string): boolean {
  const file = filePath.replace(/^\//, '');
  const pattern = patternPath.replace(/^\//, '');

  // Exact match
  if (file === pattern) return true;

  // Folder match: pattern is a prefix
  if (file.startsWith(pattern + '/')) return true;
  if (file.startsWith(pattern) && pattern.endsWith('/')) return true;

  // Parent directory match (pattern is an index file like folder/index.md)
  const patternDir = pattern.substring(0, pattern.lastIndexOf('/'));
  if (patternDir && file.startsWith(patternDir + '/')) return true;

  return false;
}

/**
 * Check if a domain matches a pattern (exact or suffix).
 */
function domainMatches(domain: string, pattern: string): boolean {
  const d = domain.toLowerCase();
  const p = pattern.toLowerCase();
  return d === p || d.endsWith('.' + p);
}

type CheckResult = 'allowed' | 'denied' | 'not_configured';

/**
 * Check a file permission against a rule.
 */
function checkFileRule(rule: PermissionRule, filePath: string): CheckResult {
  const path = filePath.replace(/^\//, '');

  // Check excludes first
  for (const scope of rule.exclude) {
    const trimmed = scope.trim();
    if (trimmed.toLowerCase() === 'all') return 'denied';
    const p = extractPathFromScope(trimmed);
    if (p && pathMatches(path, p)) return 'denied';
  }

  // Check includes
  for (const scope of rule.include) {
    const trimmed = scope.trim();
    if (trimmed.toLowerCase() === 'all') return 'allowed';
    const p = extractPathFromScope(trimmed);
    if (p && pathMatches(path, p)) return 'allowed';
  }

  return 'not_configured';
}

/**
 * Check an HTTP permission against a rule.
 */
function checkHttpRule(rule: PermissionRule, url: string): CheckResult {
  const domain = extractDomain(url);

  for (const scope of rule.exclude) {
    const trimmed = scope.trim();
    if (trimmed.toLowerCase() === 'all') return 'denied';
    if (domainMatches(domain, trimmed)) return 'denied';
  }

  for (const scope of rule.include) {
    const trimmed = scope.trim();
    if (trimmed.toLowerCase() === 'all') return 'allowed';
    if (domainMatches(domain, trimmed)) return 'allowed';
  }

  return 'not_configured';
}

/**
 * Check a storage permission against a rule.
 */
function checkStorageRule(rule: PermissionRule): CheckResult {
  for (const scope of rule.exclude) {
    if (scope.trim().toLowerCase() === 'all') return 'denied';
  }
  for (const scope of rule.include) {
    if (scope.trim().toLowerCase() === 'all') return 'allowed';
  }
  return 'not_configured';
}

/**
 * Check a permission against the workspace plugin config.
 */
function checkPermission(
  pluginsConfig: Record<string, PluginConfig> | undefined,
  pluginId: string,
  permissionType: PermissionType,
  target: string,
): CheckResult {
  if (!pluginsConfig) return 'not_configured';

  const config = pluginsConfig[pluginId];
  if (!config) return 'not_configured';

  const rule = config.permissions[permissionType];
  if (!rule) return 'not_configured';

  if (permissionType === 'http_requests') return checkHttpRule(rule, target);
  if (permissionType === 'plugin_storage') return checkStorageRule(rule);
  return checkFileRule(rule, target);
}

// ============================================================================
// Request handling
// ============================================================================

/**
 * Request a permission check. This is the main entry point for host functions.
 *
 * 1. Check the workspace config
 * 2. Check the session cache
 * 3. If not configured, create a pending request and wait for user response
 *
 * Returns true if allowed, false if denied.
 */
async function requestPermission(
  pluginId: string,
  pluginName: string,
  permissionType: PermissionType,
  target: string,
  pluginsConfig?: Record<string, PluginConfig>,
): Promise<boolean> {
  if (autoAllowAll) return true;
  const effectiveConfig = pluginsConfig ?? persistenceHandlers?.getPluginsConfig();

  // 1. Check static config
  const configResult = checkPermission(
    effectiveConfig,
    pluginId,
    permissionType,
    target,
  );
  if (configResult === 'allowed') return true;
  if (configResult === 'denied') return false;

  // 2. Plugin storage is sandboxed per-plugin — always allow.
  if (permissionType === 'plugin_storage') return true;

  // 3. Check session cache
  const key = cacheKey(pluginId, permissionType, target);
  if (key in sessionCache) {
    return sessionCache[key];
  }

  // 4. Show banner and wait for user response
  return new Promise<boolean>((resolve) => {
    const id = `perm-${++requestCounter}`;
    const request: PermissionRequest = {
      id,
      pluginId,
      pluginName,
      permissionType,
      target,
      resolve,
    };
    pendingRequests = [...pendingRequests, request];
  });
}

/**
 * Resolve a pending permission request with an ephemeral decision.
 */
function resolveRequest(requestId: string, allowed: boolean): void {
  const request = pendingRequests.find((r) => r.id === requestId);
  if (!request) return;

  // Cache the ephemeral decision
  const key = cacheKey(request.pluginId, request.permissionType, request.target);
  sessionCache = { ...sessionCache, [key]: allowed };

  // Remove from pending and resolve
  pendingRequests = pendingRequests.filter((r) => r.id !== requestId);
  request.resolve(allowed);
}

function getTargetScopeForRequest(
  request: PermissionRequest,
  mode: 'target' | 'folder',
): string {
  if (request.permissionType === 'http_requests') {
    return extractDomain(request.target);
  }

  if (request.permissionType === 'plugin_storage') {
    return 'all';
  }

  const normalized = request.target.replace(/^\//, '');
  if (!normalized) return normalized;
  if (mode === 'target') return normalized;
  const slash = normalized.lastIndexOf('/');
  if (slash <= 0) return normalized;
  return normalized.slice(0, slash);
}

async function persistRequestDecision(
  requestId: string,
  mode: 'allow_target' | 'allow_folder' | 'block_target',
): Promise<void> {
  const request = pendingRequests.find((r) => r.id === requestId);
  if (!request) return;

  const handlers = persistenceHandlers;
  if (!handlers) {
    resolveRequest(requestId, mode !== 'block_target');
    return;
  }

  const currentConfig = handlers.getPluginsConfig() ?? {};
  const pluginConfig: PluginConfig = {
    download: currentConfig[request.pluginId]?.download,
    permissions: {
      ...(currentConfig[request.pluginId]?.permissions ?? {}),
    },
  };

  const existingRule = pluginConfig.permissions[request.permissionType] ?? {
    include: [],
    exclude: [],
  };
  const nextRule: PermissionRule = {
    include: [...existingRule.include],
    exclude: [...existingRule.exclude],
  };

  if (mode === 'allow_target') {
    const targetScope = getTargetScopeForRequest(request, 'target');
    if (targetScope && !nextRule.include.includes(targetScope)) {
      nextRule.include.push(targetScope);
    }
    nextRule.exclude = nextRule.exclude.filter((s) => s !== targetScope);
  } else if (mode === 'allow_folder') {
    const folderScope = getTargetScopeForRequest(request, 'folder');
    if (folderScope && !nextRule.include.includes(folderScope)) {
      nextRule.include.push(folderScope);
    }
    nextRule.exclude = nextRule.exclude.filter((s) => s !== folderScope);
  } else {
    const targetScope = getTargetScopeForRequest(request, 'target');
    if (targetScope && !nextRule.exclude.includes(targetScope)) {
      nextRule.exclude.push(targetScope);
    }
    nextRule.include = nextRule.include.filter((s) => s !== targetScope);
  }

  pluginConfig.permissions = {
    ...pluginConfig.permissions,
    [request.permissionType]: nextRule,
  };

  const nextConfig: Record<string, PluginConfig> = {
    ...currentConfig,
    [request.pluginId]: pluginConfig,
  };

  try {
    await handlers.savePluginsConfig(nextConfig);
    resolveRequest(requestId, mode !== 'block_target');
  } catch {
    // Fallback to ephemeral decision if persistence fails.
    resolveRequest(requestId, mode !== 'block_target');
  }
}

/**
 * Dismiss a request (deny without caching).
 */
function dismissRequest(requestId: string): void {
  const request = pendingRequests.find((r) => r.id === requestId);
  if (!request) return;

  pendingRequests = pendingRequests.filter((r) => r.id !== requestId);
  request.resolve(false);
}

/**
 * Clear the session cache (e.g., on workspace switch).
 */
function clearSessionCache(): void {
  sessionCache = {};
}

function setPersistenceHandlers(
  handlers: PermissionPersistenceHandlers | null,
): void {
  persistenceHandlers = handlers;
}

// ============================================================================
// Human-readable labels
// ============================================================================

function getPermissionLabel(permType: PermissionType): string {
  return PERMISSION_LABELS[permType] ?? permType;
}

function formatTarget(permType: PermissionType, target: string): string {
  if (permType === 'http_requests') {
    return extractDomain(target);
  }
  if (permType === 'plugin_storage') {
    return 'plugin storage';
  }
  // For files, show the path (truncate if long)
  const path = target.replace(/^\//, '');
  if (path.length > 60) {
    return '...' + path.slice(-57);
  }
  return `"${path}"`;
}

// ============================================================================
// Store Export
// ============================================================================

export function getPermissionStore() {
  return {
    get pendingRequests() {
      return pendingRequests;
    },
    get hasPendingRequests() {
      return pendingRequests.length > 0;
    },
    requestPermission,
    resolveRequest,
    persistRequestDecision,
    dismissRequest,
    clearSessionCache,
    setPersistenceHandlers,
    checkPermission,
    getPermissionLabel,
    formatTarget,
    setAutoAllow(enabled: boolean) {
      autoAllowAll = enabled;
    },
  };
}

export const permissionStore = getPermissionStore();
