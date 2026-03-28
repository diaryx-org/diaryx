import type { Api } from "$lib/backend/api";
import type { JsonValue } from "$lib/backend/generated/serde_json/JsonValue";
import { executeProviderCommand } from "./providerRouter";

type ProviderCommandParams = Record<string, JsonValue>;

export async function executeProviderPluginCommand<T = JsonValue>(args: {
  api: Api;
  pluginId: string;
  command: string;
  params?: ProviderCommandParams;
}): Promise<T> {
  const { api, pluginId, command, params = {} } = args;

  return await executeProviderCommand<T>({
    api,
    pluginId,
    command,
    params,
  });
}
