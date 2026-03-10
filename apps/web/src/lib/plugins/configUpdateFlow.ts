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

  const frontmatter = await api.getFrontmatter(workspacePath)
  const existingPlugins =
    frontmatter.plugins && typeof frontmatter.plugins === "object" && !Array.isArray(frontmatter.plugins)
      ? (frontmatter.plugins as Record<string, Record<string, unknown>>)
      : {}
  const nextPlugins = applyPluginPermissionsPatch(existingPlugins, pluginId, patch)
  if (!nextPlugins) {
    return
  }

  await api.setFrontmatterProperty(
    workspacePath,
    "plugins",
    nextPlugins as unknown as JsonValue,
    workspacePath,
  )
}
