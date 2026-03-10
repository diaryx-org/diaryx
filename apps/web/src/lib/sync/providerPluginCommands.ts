import type { Api } from "$lib/backend/api";
import type { JsonValue } from "$lib/backend/generated/serde_json/JsonValue";

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

export async function executeProviderPluginCommand<T = JsonValue>(args: {
  api: Api;
  pluginId: string;
  command: string;
  params?: ProviderCommandParams;
}): Promise<T> {
  const { api, pluginId, command, params = {} } = args;

  return await api.executePluginCommand(
    pluginId,
    command,
    buildProviderCommandParams(pluginId, params) as JsonValue,
  ) as T;
}
