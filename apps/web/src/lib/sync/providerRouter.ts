import type { Api } from "$lib/backend/api";
import type { JsonValue } from "$lib/backend/generated/serde_json/JsonValue";
import { isBuiltinProvider } from "./builtinProviders";
import { executeBuiltinIcloudProviderCommand } from "./builtinIcloudProvider";

type ProviderCommandParams = Record<string, JsonValue>;

function buildProviderCommandParams(
  pluginId: string,
  params: ProviderCommandParams = {},
): ProviderCommandParams {
  return {
    provider_id: pluginId,
    ...params,
  };
}

export async function executeProviderCommand<T = JsonValue>(args: {
  api: Api;
  pluginId: string;
  command: string;
  params?: ProviderCommandParams;
}): Promise<T> {
  const { api, pluginId, command, params = {} } = args;

  if (isBuiltinProvider(pluginId)) {
    if (pluginId === "builtin.icloud") {
      return await executeBuiltinIcloudProviderCommand<T>({
        api,
        command,
        params: buildProviderCommandParams(pluginId, params),
      });
    }

    throw new Error(`Unsupported built-in provider: ${pluginId}`);
  }

  return await api.executePluginCommand(
    pluginId,
    command,
    buildProviderCommandParams(pluginId, params) as JsonValue,
  ) as T;
}
