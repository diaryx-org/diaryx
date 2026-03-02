<script lang="ts">
  /**
   * GoogleDriveStorageSettings - Configure Google Drive as a filesystem backend.
   *
   * OAuth sign-in stays in Svelte (requires browser interaction). After sign-in,
   * tokens are passed to the diaryx.storage.gdrive Extism plugin via SetConfig.
   * The plugin handles token refresh internally.
   */
  import { Button } from "$lib/components/ui/button";
  import { Label } from "$lib/components/ui/label";
  import { Loader2, Check, AlertCircle, LogOut } from "@lucide/svelte";
  import { dispatchCommand } from "$lib/plugins/browserPluginManager.svelte";

  const PLUGIN_ID = "diaryx.storage.gdrive";

  // OAuth config — these should be provided by the app
  let clientId = $state("");
  let clientSecret = $state("");
  let rootFolderId = $state("root");

  // Auth state
  let isAuthenticated = $state(false);
  let refreshing = $state(false);
  let configLoaded = $state(false);

  // UI state
  let statusMessage: { success: boolean; message: string } | null = $state(null);

  // Load existing config on mount
  $effect(() => {
    if (!configLoaded) {
      loadConfig();
    }
  });

  async function loadConfig() {
    try {
      const result = await dispatchCommand(PLUGIN_ID, "GetConfig", {});
      if (result.success && result.data) {
        const config = result.data as Record<string, unknown>;
        clientId = (config.client_id as string) ?? "";
        clientSecret = (config.client_secret as string) ?? "";
        rootFolderId = (config.root_folder_id as string) ?? "root";
        // Check if we have valid tokens
        const hasToken = config.refresh_token && config.refresh_token !== "";
        isAuthenticated = !!hasToken;
      }
    } catch {
      // Plugin not loaded yet
    }
    configLoaded = true;
  }

  async function startOAuthFlow() {
    if (!clientId) {
      statusMessage = { success: false, message: "Client ID is required" };
      return;
    }

    // Open Google OAuth consent screen
    const redirectUri = `${window.location.origin}/oauth/callback`;
    const scope = encodeURIComponent("https://www.googleapis.com/auth/drive.file");
    const authUrl =
      `https://accounts.google.com/o/oauth2/v2/auth?client_id=${encodeURIComponent(clientId)}` +
      `&redirect_uri=${encodeURIComponent(redirectUri)}` +
      `&response_type=code&scope=${scope}&access_type=offline&prompt=consent`;

    // Open popup for OAuth
    const popup = window.open(authUrl, "google-oauth", "width=500,height=600");
    if (!popup) {
      statusMessage = { success: false, message: "Popup blocked — please allow popups" };
      return;
    }

    // Listen for the callback
    const handler = async (event: MessageEvent) => {
      if (event.origin !== window.location.origin) return;
      if (event.data?.type !== "oauth-callback") return;

      window.removeEventListener("message", handler);

      const code = event.data.code;
      if (!code) {
        statusMessage = { success: false, message: "No authorization code received" };
        return;
      }

      // Exchange code for tokens
      try {
        const tokenResp = await fetch("https://oauth2.googleapis.com/token", {
          method: "POST",
          headers: { "Content-Type": "application/x-www-form-urlencoded" },
          body: new URLSearchParams({
            code,
            client_id: clientId,
            client_secret: clientSecret,
            redirect_uri: redirectUri,
            grant_type: "authorization_code",
          }),
        });
        const tokens = await tokenResp.json();
        if (tokens.error) {
          statusMessage = { success: false, message: tokens.error_description ?? tokens.error };
          return;
        }

        // Save tokens to plugin
        await dispatchCommand(PLUGIN_ID, "SetConfig", {
          access_token: tokens.access_token,
          refresh_token: tokens.refresh_token,
          client_id: clientId,
          client_secret: clientSecret,
          root_folder_id: rootFolderId,
        });

        isAuthenticated = true;
        statusMessage = { success: true, message: "Connected to Google Drive" };
      } catch (e) {
        statusMessage = { success: false, message: `Token exchange failed: ${e}` };
      }
    };

    window.addEventListener("message", handler);
  }

  async function refreshToken() {
    refreshing = true;
    try {
      const result = await dispatchCommand(PLUGIN_ID, "RefreshToken", {});
      if (result.success) {
        statusMessage = { success: true, message: "Token refreshed" };
      } else {
        statusMessage = { success: false, message: result.error ?? "Refresh failed" };
      }
    } catch (e) {
      statusMessage = { success: false, message: String(e) };
    }
    refreshing = false;
  }

  async function disconnect() {
    await dispatchCommand(PLUGIN_ID, "SetConfig", {
      access_token: "",
      refresh_token: "",
      client_id: clientId,
      client_secret: clientSecret,
      root_folder_id: "root",
    });
    isAuthenticated = false;
    statusMessage = { success: true, message: "Disconnected from Google Drive" };
  }

  async function saveConfig() {
    try {
      // Only save non-token config if not authenticated
      if (!isAuthenticated) {
        await dispatchCommand(PLUGIN_ID, "SetConfig", {
          access_token: "",
          refresh_token: "",
          client_id: clientId,
          client_secret: clientSecret,
          root_folder_id: rootFolderId,
        });
      }
      statusMessage = { success: true, message: "Configuration saved" };
    } catch (e) {
      statusMessage = { success: false, message: String(e) };
    }
  }
</script>

<div class="space-y-4">
  <div class="space-y-3">
    <div class="space-y-1">
      <Label for="gdrive-client-id">Client ID</Label>
      <input
        id="gdrive-client-id"
        type="text"
        bind:value={clientId}
        placeholder="your-client-id.apps.googleusercontent.com"
        class="w-full rounded-md border border-input bg-background px-3 py-2 text-sm"
        disabled={isAuthenticated}
      />
    </div>

    <div class="space-y-1">
      <Label for="gdrive-client-secret">Client Secret</Label>
      <input
        id="gdrive-client-secret"
        type="password"
        bind:value={clientSecret}
        placeholder="••••••••"
        class="w-full rounded-md border border-input bg-background px-3 py-2 text-sm"
        disabled={isAuthenticated}
      />
    </div>

    <div class="space-y-1">
      <Label for="gdrive-folder-id">Root Folder ID</Label>
      <input
        id="gdrive-folder-id"
        type="text"
        bind:value={rootFolderId}
        placeholder="root"
        class="w-full rounded-md border border-input bg-background px-3 py-2 text-sm"
      />
      <p class="text-xs text-muted-foreground">
        Google Drive folder ID to use as root. "root" uses the top-level drive.
      </p>
    </div>
  </div>

  {#if statusMessage}
    <div class="flex items-center gap-2 text-sm" class:text-green-600={statusMessage.success} class:text-destructive={!statusMessage.success}>
      {#if statusMessage.success}
        <Check class="size-4" />
      {:else}
        <AlertCircle class="size-4" />
      {/if}
      {statusMessage.message}
    </div>
  {/if}

  <div class="flex gap-2">
    {#if isAuthenticated}
      <Button variant="outline" onclick={refreshToken} disabled={refreshing}>
        {#if refreshing}
          <Loader2 class="size-4 mr-1.5 animate-spin" />
        {/if}
        Refresh Token
      </Button>
      <Button variant="destructive" onclick={disconnect}>
        <LogOut class="size-4 mr-1.5" />
        Disconnect
      </Button>
    {:else}
      <Button variant="outline" onclick={saveConfig} disabled={!clientId}>
        Save
      </Button>
      <Button onclick={startOAuthFlow} disabled={!clientId || !clientSecret}>
        Sign in with Google
      </Button>
    {/if}
  </div>
</div>
