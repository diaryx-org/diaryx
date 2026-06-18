import type { Api } from "$lib/backend/api"
import type { JsonValue } from "$lib/backend/generated/serde_json/JsonValue"
import { dispatchCommand, getPlugin } from "$lib/plugins/browserPluginManager.svelte"

type PluginCommandResult = {
  success: boolean
  data?: unknown
  error?: string
}

type PermissionRulePatch = {
  include?: string[]
  exclude?: string[]
}

type PluginPermissionsPatch = {
  plugin_id?: string
  mode?: "merge" | "replace"
  permissions: Record<string, PermissionRulePatch>
}

export function readPluginPermissionsPatch(data: unknown): PluginPermissionsPatch | null {
  if (!data || typeof data !== "object" || !("plugin_permissions_patch" in data)) {
    return null
  }
  const patch = (data as { plugin_permissions_patch?: unknown }).plugin_permissions_patch
  if (!patch || typeof patch !== "object" || Array.isArray(patch)) {
    return null
  }
  if (!("permissions" in patch)) {
    return null
  }
  return patch as PluginPermissionsPatch
}

function asStringArray(value: unknown): string[] {
  if (!Array.isArray(value)) return []
  return value.filter((entry): entry is string => typeof entry === "string")
}

function mergeStringArrays(existing: string[], incoming: string[]): string[] {
  const merged = [...existing]
  for (const value of incoming) {
    if (!merged.includes(value)) {
      merged.push(value)
    }
  }
  return merged
}

export function applyPluginPermissionsPatch(
  existingPlugins: Record<string, Record<string, unknown>>,
  pluginId: string,
  patch: PluginPermissionsPatch,
): Record<string, Record<string, unknown>> | null {
  const effectivePluginId =
    typeof patch.plugin_id === "string" && patch.plugin_id.trim().length > 0
      ? patch.plugin_id
      : pluginId
  const mode = patch.mode === "merge" ? "merge" : "replace"
  const currentPlugin = { ...(existingPlugins[effectivePluginId] ?? {}) }
  const currentPermissions =
    currentPlugin.permissions && typeof currentPlugin.permissions === "object"
      ? { ...(currentPlugin.permissions as Record<string, unknown>) }
      : {}

  let changed = false
  for (const [permissionType, rulePatch] of Object.entries(patch.permissions ?? {})) {
    const include = asStringArray(rulePatch?.include)
    const exclude = asStringArray(rulePatch?.exclude)
    const existingRule =
      currentPermissions[permissionType] && typeof currentPermissions[permissionType] === "object"
        ? (currentPermissions[permissionType] as Record<string, unknown>)
        : {}

    const nextRule =
      mode === "merge"
        ? {
            include: mergeStringArrays(asStringArray(existingRule.include), include),
            exclude: mergeStringArrays(asStringArray(existingRule.exclude), exclude),
          }
        : {
            include,
            exclude,
          }

    if (JSON.stringify(existingRule) !== JSON.stringify(nextRule)) {
      currentPermissions[permissionType] = nextRule
      changed = true
    }
  }

  if (!changed) {
    return null
  }

  return {
    ...existingPlugins,
    [effectivePluginId]: {
      ...currentPlugin,
      permissions: currentPermissions,
    },
  }
}

async function executePluginCommand(
  pluginId: string,
  command: string,
  params: Record<string, JsonValue>,
  api?: Api | null,
): Promise<PluginCommandResult> {
  const browserPlugin = getPlugin(pluginId)
  if (browserPlugin) {
    return dispatchCommand(pluginId, command, params)
  }

  if (!api) {
    return {
      success: false,
      error: `Plugin command unavailable: ${pluginId}`,
    }
  }

  try {
    const data = await api.executePluginCommand(pluginId, command, params as JsonValue)
    return { success: true, data }
  } catch (error) {
    return {
      success: false,
      error: error instanceof Error ? error.message : String(error),
    }
  }
}

function isMissingUpdateConfigError(pluginId: string, error?: string): boolean {
  if (!error) return false
  return [
    "Unknown command: UpdateConfig",
    `No plugin '${pluginId}' handles command 'UpdateConfig'`,
    `Plugin '${pluginId}' not found`,
    `Plugin not loaded: ${pluginId}`,
    `Plugin command unavailable: ${pluginId}`,
  ].some((message) => error.includes(message))
}

/** A plugin's request to re-scope its granted permissions, surfaced for approval. */
export type PluginPermissionRequest = {
  permissions: Record<string, PermissionRulePatch>
  reasons?: Record<string, string>
}

/** The host's pending reconcile returned from `setPluginConfig`. */
export type PendingPluginReconcile = {
  permission_request?: PluginPermissionRequest | null
  migrations?: unknown[]
}

function readPendingReconcile(data: unknown): PendingPluginReconcile | null {
  if (!data || typeof data !== "object" || Array.isArray(data)) return null
  return data as PendingPluginReconcile
}

/** Format a human-readable consent prompt for a permission request. */
function formatPermissionConsent(
  pluginId: string,
  permissions: Record<string, PermissionRulePatch>,
  reasons?: Record<string, string>,
): string {
  const lines = [`The "${pluginId}" plugin needs these file-access changes:`, ""]
  for (const [category, rule] of Object.entries(permissions)) {
    const include = asStringArray(rule?.include)
    lines.push(`• ${category}: ${include.length ? include.join(", ") : "(none)"}`)
    const reason = reasons?.[category]
    if (reason) lines.push(`    ${reason}`)
  }
  lines.push("", "Approve these permissions?")
  return lines.join("\n")
}

/**
 * Surface a plugin's permission request for explicit user approval, then apply
 * it to `plugins.<id>.permissions`.
 *
 * Security: the request is clamped to the permission categories the plugin was
 * already granted (its install-approved ceiling) — a request introducing a new
 * category is dropped, not silently granted. The change is then shown in a
 * blocking confirm and only written on approval. Returns whether it was applied.
 */
export async function applyPermissionRequestWithConsent(args: {
  pluginId: string
  request: PluginPermissionRequest
  api: Api
  workspacePath: string
}): Promise<boolean> {
  const { pluginId, request, api, workspacePath } = args
  const requested = request?.permissions
  if (!requested || Object.keys(requested).length === 0) return false

  const wsConfig = await api.getWorkspaceConfig(workspacePath)
  const existingPlugins =
    wsConfig.plugins && typeof wsConfig.plugins === "object" && !Array.isArray(wsConfig.plugins)
      ? (wsConfig.plugins as Record<string, Record<string, unknown>>)
      : {}

  // Ceiling clamp: only categories the plugin already has granted may change.
  // When the plugin has no granted permissions yet (e.g. first run), allow the
  // request — the confirm below is still the gate.
  const granted = existingPlugins[pluginId]?.permissions
  const grantedCategories =
    granted && typeof granted === "object" && !Array.isArray(granted)
      ? new Set(Object.keys(granted as Record<string, unknown>))
      : null

  const clamped: Record<string, PermissionRulePatch> = {}
  for (const [category, rule] of Object.entries(requested)) {
    if (!grantedCategories || grantedCategories.has(category)) {
      clamped[category] = rule
    } else {
      console.warn(
        `[configUpdateFlow] dropping permission category '${category}' for ${pluginId}: not previously granted`,
      )
    }
  }
  if (Object.keys(clamped).length === 0) return false

  // Compute the resulting mapping; bail if nothing actually changes (avoids a
  // pointless prompt when the granted scope already matches).
  const nextPlugins = applyPluginPermissionsPatch(existingPlugins, pluginId, {
    plugin_id: pluginId,
    mode: "replace",
    permissions: clamped,
  })
  if (!nextPlugins) return false

  const approved =
    typeof window !== "undefined" &&
    window.confirm(formatPermissionConsent(pluginId, clamped, request.reasons))
  if (!approved) return false

  await api.setWorkspaceConfig(workspacePath, "plugins", JSON.stringify(nextPlugins))
  return true
}

/**
 * Save a plugin's declarative config to `plugins.<id>.config` and surface any
 * permission request the host hands back for user approval.
 *
 * This is the new path for plugins that opt into host-managed config (e.g.
 * the daily plugin's entry folder). The host persists the config; permission
 * changes go through {@link applyPermissionRequestWithConsent}.
 */
export async function savePluginDeclarativeConfig(args: {
  pluginId: string
  config: JsonValue
  api: Api
  workspacePath: string | null
}): Promise<void> {
  const { pluginId, config, api, workspacePath } = args
  const pending = readPendingReconcile(await api.setPluginConfig(pluginId, config))
  if (!workspacePath) return
  const request = pending?.permission_request
  if (request) {
    await applyPermissionRequestWithConsent({ pluginId, request, api, workspacePath })
  }
  // Migrations (tier-3, root-index legacy keys) are surfaced via the open-time
  // migration prompt, not here.
}

export async function runPluginUpdateConfigFlow(args: {
  pluginId: string
  params: Record<string, JsonValue>
  api?: Api | null
  workspacePath?: string | null
}): Promise<void> {
  const { pluginId, params, api = null, workspacePath = null } = args
  const result = await executePluginCommand(pluginId, "UpdateConfig", params, api)

  if (!result.success) {
    if (!isMissingUpdateConfigError(pluginId, result.error)) {
      console.warn(`[configUpdateFlow] UpdateConfig failed for ${pluginId}:`, result.error)
    }
    return
  }

  const patch = readPluginPermissionsPatch(result.data)
  if (!patch || !api || !workspacePath) {
    return
  }

  const wsConfig = await api.getWorkspaceConfig(workspacePath)
  const existingPlugins =
    wsConfig.plugins && typeof wsConfig.plugins === "object" && !Array.isArray(wsConfig.plugins)
      ? (wsConfig.plugins as Record<string, Record<string, unknown>>)
      : {}
  const nextPlugins = applyPluginPermissionsPatch(existingPlugins, pluginId, patch)
  if (!nextPlugins) {
    return
  }

  await api.setWorkspaceConfig(
    workspacePath,
    "plugins",
    JSON.stringify(nextPlugins),
  )
}
