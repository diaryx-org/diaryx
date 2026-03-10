import { isTauri } from "$lib/backend/interface";

const SECRET_PREFIX = "diaryx-plugin-secret";

function secretKey(pluginId: string, key: string): string {
  return `${SECRET_PREFIX}:${pluginId}:${key}`;
}

export async function getPluginSecret(
  pluginId: string,
  key: string,
): Promise<string | null> {
  const resolvedKey = secretKey(pluginId, key);

  if (isTauri()) {
    try {
      const { getCredential } = await import("$lib/credentials");
      return await getCredential(resolvedKey);
    } catch (error) {
      console.warn("[pluginSecretStore] falling back to localStorage get", {
        pluginId,
        key,
        error,
      });
    }
  }

  try {
    return localStorage.getItem(resolvedKey);
  } catch {
    return null;
  }
}

export async function setPluginSecret(
  pluginId: string,
  key: string,
  value: string,
): Promise<void> {
  const resolvedKey = secretKey(pluginId, key);

  if (isTauri()) {
    try {
      const { storeCredential } = await import("$lib/credentials");
      await storeCredential(resolvedKey, value);
      return;
    } catch (error) {
      console.warn("[pluginSecretStore] falling back to localStorage set", {
        pluginId,
        key,
        error,
      });
    }
  }

  localStorage.setItem(resolvedKey, value);
}

export async function deletePluginSecret(
  pluginId: string,
  key: string,
): Promise<void> {
  const resolvedKey = secretKey(pluginId, key);

  if (isTauri()) {
    try {
      const { removeCredential } = await import("$lib/credentials");
      await removeCredential(resolvedKey);
      return;
    } catch (error) {
      console.warn("[pluginSecretStore] falling back to localStorage delete", {
        pluginId,
        key,
        error,
      });
    }
  }

  localStorage.removeItem(resolvedKey);
}
