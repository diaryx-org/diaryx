import { isTauri } from "$lib/backend/interface";

export interface OpenOauthPayload {
  url: string;
  redirect_uri_prefix?: string;
}

export interface OpenOauthResult {
  code: string;
  redirect_uri: string;
}

export async function openOauthWindow(
  payload: OpenOauthPayload,
): Promise<OpenOauthResult> {
  const oauthUrl = payload.url?.trim();
  if (!oauthUrl) {
    throw new Error("open-oauth requires payload.url");
  }

  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    const redirectPrefix =
      typeof payload.redirect_uri_prefix === "string" &&
      payload.redirect_uri_prefix.length > 0
        ? payload.redirect_uri_prefix
        : "http://localhost/oauth/callback";
    const result = await invoke<{ code: string }>("oauth_webview", {
      url: oauthUrl,
      redirectPrefix,
    });
    return { code: result.code, redirect_uri: redirectPrefix };
  }

  const popup = window.open(oauthUrl, "oauth-popup", "width=500,height=600");
  if (!popup) {
    throw new Error("Popup blocked — please allow popups");
  }

  return new Promise((resolve, reject) => {
    const timeout = window.setTimeout(() => {
      window.removeEventListener("message", handler);
      reject(new Error("OAuth timed out"));
    }, 120_000);

    const handler = (event: MessageEvent) => {
      if (event.origin !== window.location.origin) return;
      if (event.data?.type !== "oauth-callback") return;
      window.clearTimeout(timeout);
      window.removeEventListener("message", handler);
      if (event.data.error) {
        reject(new Error(String(event.data.error)));
        return;
      }
      resolve({
        code: String(event.data.code ?? ""),
        redirect_uri: `${window.location.origin}/oauth/callback`,
      });
    };

    window.addEventListener("message", handler);
  });
}
