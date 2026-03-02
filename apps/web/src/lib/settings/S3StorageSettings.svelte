<script lang="ts">
  /**
   * S3StorageSettings - Configure S3-compatible storage as a filesystem backend.
   *
   * This component dispatches commands to the diaryx.storage.s3 Extism plugin
   * rather than using Tauri-specific S3 API calls.
   */
  import { Button } from "$lib/components/ui/button";
  import { Label } from "$lib/components/ui/label";
  import { Loader2, Check, AlertCircle } from "@lucide/svelte";
  import { dispatchCommand } from "$lib/plugins/browserPluginManager.svelte";

  const PLUGIN_ID = "diaryx.storage.s3";

  // Form state
  let bucket = $state("");
  let region = $state("us-east-1");
  let prefix = $state("");
  let endpoint = $state("");
  let accessKeyId = $state("");
  let secretAccessKey = $state("");
  let pathStyle = $state(false);

  // UI state
  let testing = $state(false);
  let testResult: { success: boolean; message: string } | null = $state(null);
  let saving = $state(false);
  let configLoaded = $state(false);

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
        bucket = (config.bucket as string) ?? "";
        region = (config.region as string) ?? "us-east-1";
        prefix = (config.prefix as string) ?? "";
        endpoint = (config.endpoint as string) ?? "";
        accessKeyId = (config.access_key_id as string) ?? "";
        secretAccessKey = (config.secret_access_key as string) ?? "";
        pathStyle = (config.path_style as boolean) ?? false;
      }
    } catch {
      // Plugin not loaded yet, ignore
    }
    configLoaded = true;
  }

  async function saveConfig() {
    saving = true;
    try {
      await dispatchCommand(PLUGIN_ID, "SetConfig", {
        bucket,
        region,
        prefix,
        endpoint: endpoint || null,
        access_key_id: accessKeyId,
        secret_access_key: secretAccessKey,
        path_style: pathStyle,
      });
      testResult = { success: true, message: "Configuration saved" };
    } catch (e) {
      testResult = { success: false, message: String(e) };
    }
    saving = false;
  }

  async function testConnection() {
    testing = true;
    testResult = null;
    // Save config first so the plugin has credentials
    await saveConfig();
    try {
      const result = await dispatchCommand(PLUGIN_ID, "TestConnection", {});
      if (result.success) {
        testResult = { success: true, message: "Connection successful" };
      } else {
        testResult = { success: false, message: result.error ?? "Connection failed" };
      }
    } catch (e) {
      testResult = { success: false, message: String(e) };
    }
    testing = false;
  }
</script>

<div class="space-y-4">
  <div class="space-y-3">
    <div class="space-y-1">
      <Label for="s3-bucket">Bucket</Label>
      <input
        id="s3-bucket"
        type="text"
        bind:value={bucket}
        placeholder="my-diaryx-bucket"
        class="w-full rounded-md border border-input bg-background px-3 py-2 text-sm"
      />
    </div>

    <div class="space-y-1">
      <Label for="s3-region">Region</Label>
      <input
        id="s3-region"
        type="text"
        bind:value={region}
        placeholder="us-east-1"
        class="w-full rounded-md border border-input bg-background px-3 py-2 text-sm"
      />
    </div>

    <div class="space-y-1">
      <Label for="s3-prefix">Prefix (optional)</Label>
      <input
        id="s3-prefix"
        type="text"
        bind:value={prefix}
        placeholder="diaryx/"
        class="w-full rounded-md border border-input bg-background px-3 py-2 text-sm"
      />
      <p class="text-xs text-muted-foreground">
        Key prefix within the bucket (e.g., "diaryx/workspace1/")
      </p>
    </div>

    <div class="space-y-1">
      <Label for="s3-endpoint">Custom Endpoint (optional)</Label>
      <input
        id="s3-endpoint"
        type="text"
        bind:value={endpoint}
        placeholder="https://s3.example.com"
        class="w-full rounded-md border border-input bg-background px-3 py-2 text-sm"
      />
      <p class="text-xs text-muted-foreground">
        For S3-compatible services: MinIO, Cloudflare R2, Backblaze B2, etc.
      </p>
    </div>

    <div class="space-y-1">
      <Label for="s3-access-key">Access Key ID</Label>
      <input
        id="s3-access-key"
        type="text"
        bind:value={accessKeyId}
        placeholder="AKIAIOSFODNN7EXAMPLE"
        class="w-full rounded-md border border-input bg-background px-3 py-2 text-sm"
      />
    </div>

    <div class="space-y-1">
      <Label for="s3-secret-key">Secret Access Key</Label>
      <input
        id="s3-secret-key"
        type="password"
        bind:value={secretAccessKey}
        placeholder="••••••••"
        class="w-full rounded-md border border-input bg-background px-3 py-2 text-sm"
      />
    </div>

    <div class="flex items-center gap-2">
      <input
        id="s3-path-style"
        type="checkbox"
        bind:checked={pathStyle}
        class="rounded border-input"
      />
      <Label for="s3-path-style">Use path-style addressing</Label>
    </div>
  </div>

  {#if testResult}
    <div class="flex items-center gap-2 text-sm" class:text-green-600={testResult.success} class:text-destructive={!testResult.success}>
      {#if testResult.success}
        <Check class="size-4" />
      {:else}
        <AlertCircle class="size-4" />
      {/if}
      {testResult.message}
    </div>
  {/if}

  <div class="flex gap-2">
    <Button variant="outline" onclick={saveConfig} disabled={saving || !bucket || !accessKeyId}>
      {#if saving}
        <Loader2 class="size-4 mr-1.5 animate-spin" />
      {/if}
      Save
    </Button>
    <Button onclick={testConnection} disabled={testing || !bucket || !accessKeyId}>
      {#if testing}
        <Loader2 class="size-4 mr-1.5 animate-spin" />
      {/if}
      Test Connection
    </Button>
  </div>
</div>
