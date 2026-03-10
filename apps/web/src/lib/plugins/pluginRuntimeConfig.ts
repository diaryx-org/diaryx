import { isTauri } from "$lib/backend/interface";
import type { JsonValue } from "$lib/backend/generated/serde_json/JsonValue";

const GDRIVE_PLUGIN_ID = "diaryx.storage.gdrive";
const env = (import.meta as ImportMeta & {
  env?: Record<string, string | undefined>;
}).env ?? {};
const GDRIVE_SHARED_CLIENT_ID = env.VITE_GOOGLE_DRIVE_CLIENT_ID;
const GDRIVE_WEB_CLIENT_ID = env.VITE_GOOGLE_DRIVE_WEB_CLIENT_ID;
const GDRIVE_DESKTOP_CLIENT_ID = env.VITE_GOOGLE_DRIVE_DESKTOP_CLIENT_ID;

function trimEnv(value: string | undefined): string | null {
  const trimmed = value?.trim();
  return trimmed ? trimmed : null;
}

function resolveGoogleDriveClientId(
  fallback?: string | null,
): string | null {
  const preferred = isTauri()
    ? trimEnv(GDRIVE_DESKTOP_CLIENT_ID)
    : trimEnv(GDRIVE_WEB_CLIENT_ID);
  return preferred ?? trimEnv(GDRIVE_SHARED_CLIENT_ID) ?? fallback ?? null;
}

export function mergeRuntimePluginConfig(
  pluginId: string,
  config: Record<string, JsonValue>,
): Record<string, JsonValue> {
  if (pluginId !== GDRIVE_PLUGIN_ID) {
    return config;
  }

  const fallback =
    typeof config.client_id === "string" ? config.client_id : null;
  const clientId = resolveGoogleDriveClientId(fallback);
  if (!clientId) {
    return config;
  }

  return {
    ...config,
    client_id: clientId,
  };
}

function base64UrlEncode(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) {
    binary += String.fromCharCode(byte);
  }
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/g, "");
}

async function createPkcePair(): Promise<{
  code_challenge: string;
  code_verifier: string;
}> {
  const verifierBytes = crypto.getRandomValues(new Uint8Array(32));
  const verifier = base64UrlEncode(verifierBytes);
  const challengeBytes = new Uint8Array(
    await crypto.subtle.digest(
      "SHA-256",
      new TextEncoder().encode(verifier),
    ),
  );
  return {
    code_challenge: base64UrlEncode(challengeBytes),
    code_verifier: verifier,
  };
}

export async function getRuntimePluginCommandParams(
  pluginId: string,
  command: string,
  config: Record<string, JsonValue>,
): Promise<Record<string, JsonValue>> {
  if (pluginId !== GDRIVE_PLUGIN_ID || command !== "BeginOAuth") {
    return {};
  }

  const fallback =
    typeof config.client_id === "string" ? config.client_id : null;
  const clientId = resolveGoogleDriveClientId(fallback);
  if (!clientId) {
    return {};
  }

  const redirectUri = isTauri()
    ? "http://localhost/oauth/callback"
    : `${window.location.origin}/oauth/callback`;
  const pkce = await createPkcePair();

  return {
    client_id: clientId,
    redirect_uri: redirectUri,
    redirect_uri_prefix: redirectUri,
    ...pkce,
  };
}
