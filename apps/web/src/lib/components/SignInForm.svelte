<script lang="ts">
  import { proxyFetch } from "$lib/backend/proxyFetch";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import VerificationCodeInput from "$lib/components/VerificationCodeInput.svelte";
  import {
    Loader2,
    Mail,
    Link,
    ChevronDown,
    ChevronUp,
  } from "@lucide/svelte";
  import { getBackend, createApi, isTauri } from "$lib/backend";
  import {
    requestMagicLink,
    verifyMagicLink,
    setServerUrl,
  } from "$lib/auth";
  import { onMount } from "svelte";

  interface Props {
    onAuthenticated?: () => void;
    compact?: boolean;
  }

  let { onAuthenticated, compact = false }: Props = $props();

  const defaultServerUrl = isTauri() ? "https://sync.diaryx.org" : "/api";
  let email = $state("");
  let serverUrl = $state(
    typeof window !== "undefined"
      ? localStorage.getItem("diaryx_sync_server_url") || defaultServerUrl
      : defaultServerUrl
  );
  let showAdvanced = $state(false);
  let verificationSent = $state(false);
  let devLink = $state<string | null>(null);
  let isSending = $state(false);
  let isValidating = $state(false);
  let error = $state<string | null>(null);
  let resendCooldown = $state(0);
  let resendInterval: ReturnType<typeof setInterval> | null = null;
  let urlCheckInterval: ReturnType<typeof setInterval> | null = null;

  onMount(() => {
    return () => {
      stopMagicLinkDetection();
      if (resendInterval) clearInterval(resendInterval);
    };
  });

  async function validateServer(): Promise<boolean> {
    const backend = await getBackend();
    const api = createApi(backend);

    let url = await api.normalizeServerUrl(serverUrl);
    if (!url) { error = "Please enter a server URL"; return false; }
    serverUrl = url;

    isValidating = true;
    error = null;
    try {
      const resp = await proxyFetch(`${url}/health`, { method: "GET", timeout_ms: 5000 });
      if (!resp.ok) throw new Error("Server returned an error");
      setServerUrl(url);
      return true;
    } catch (e) {
      error = e instanceof Error && e.name === "TimeoutError"
        ? "Connection timed out. Check the URL and try again."
        : "Could not connect to server. Please check the URL.";
      return false;
    } finally { isValidating = false; }
  }

  async function handleSendMagicLink() {
    if (!email.trim()) { error = "Please enter your email address"; return; }
    if (!(await validateServer())) return;
    isSending = true;
    error = null;
    devLink = null;
    try {
      const result = await requestMagicLink(email.trim());
      devLink = result.devLink || null;
      verificationSent = true;
      startMagicLinkDetection();
      startResendCooldown();
    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to send magic link";
    } finally { isSending = false; }
  }

  function startMagicLinkDetection() {
    stopMagicLinkDetection();
    urlCheckInterval = setInterval(async () => {
      const params = new URLSearchParams(window.location.search);
      const token = params.get("token");
      if (token) {
        stopMagicLinkDetection();
        window.history.replaceState({}, "", location.pathname);
        await handleVerifyToken(token);
      }
    }, 1000);
  }

  function stopMagicLinkDetection() {
    if (urlCheckInterval) { clearInterval(urlCheckInterval); urlCheckInterval = null; }
  }

  function startResendCooldown() {
    resendCooldown = 60;
    if (resendInterval) clearInterval(resendInterval);
    resendInterval = setInterval(() => {
      resendCooldown--;
      if (resendCooldown <= 0 && resendInterval) {
        clearInterval(resendInterval);
        resendInterval = null;
      }
    }, 1000);
  }

  async function handleVerifyToken(token: string) {
    if (!token.trim()) { error = "Invalid verification token"; return; }
    error = null;
    try {
      await verifyMagicLink(token.trim());
      verificationSent = false;
      email = "";
      onAuthenticated?.();
    } catch (e) {
      error = e instanceof Error ? e.message : "Verification failed";
    }
  }
</script>

<div class="space-y-3">
  {#if error}
    <p class="text-xs text-destructive">{error}</p>
  {/if}

  {#if !verificationSent}
    {#if !compact}
      <p class="text-sm text-muted-foreground">
        Sign in to enable sync across devices.
      </p>
    {/if}

    <div class="space-y-2">
      <Label for="signin-email" class="text-sm">Email Address</Label>
      <Input
        id="signin-email"
        type="email"
        bind:value={email}
        placeholder="you@example.com"
        disabled={isSending || isValidating}
        onkeydown={(e) => e.key === "Enter" && handleSendMagicLink()}
      />
    </div>

    <div>
      <Button
        variant="ghost"
        size="sm"
        class="w-full justify-between"
        onclick={() => showAdvanced = !showAdvanced}
      >
        <span>Advanced</span>
        {#if showAdvanced}
          <ChevronUp class="size-4" />
        {:else}
          <ChevronDown class="size-4" />
        {/if}
      </Button>
      {#if showAdvanced}
        <div class="space-y-2 mt-2">
          <Label for="signin-server-url" class="text-sm">Server URL</Label>
          <Input
            id="signin-server-url"
            type="text"
            bind:value={serverUrl}
            placeholder="https://sync.diaryx.org"
            disabled={isSending || isValidating}
          />
        </div>
      {/if}
    </div>

    <Button
      class="w-full"
      onclick={handleSendMagicLink}
      disabled={isSending || isValidating || !email.trim()}
    >
      {#if isSending || isValidating}
        <Loader2 class="size-4 mr-2 animate-spin" />
        {isValidating ? 'Connecting...' : 'Sending...'}
      {:else}
        <Mail class="size-4 mr-2" />
        Send Sign-in Link
      {/if}
    </Button>
  {:else}
    <div class="space-y-4">
      {#if devLink}
        <div class="space-y-2 p-3 bg-amber-500/10 rounded-md">
          <p class="text-xs text-amber-700 dark:text-amber-400 font-medium">
            Development mode: Email not configured
          </p>
          <a
            href={devLink}
            class="text-xs text-primary hover:underline flex items-center gap-1 break-all"
            onclick={(e) => {
              e.preventDefault();
              handleVerifyToken(new URL(devLink!).searchParams.get("token") || "");
            }}
          >
            <Link class="size-3 shrink-0" />
            Click here to verify
          </a>
        </div>
      {:else}
        <div class="text-center space-y-2 py-4">
          <Mail class="size-12 mx-auto text-muted-foreground" />
          <p class="text-sm font-medium">
            Check your email at <span class="text-primary">{email}</span>
          </p>
          <p class="text-xs text-muted-foreground">
            Click the link in your email to continue.
          </p>
        </div>

        <VerificationCodeInput
          {email}
          onVerified={() => {
            verificationSent = false;
            stopMagicLinkDetection();
            email = "";
            onAuthenticated?.();
          }}
          onError={(msg) => { error = msg; }}
        />

        <div class="flex justify-center">
          <Button
            variant="outline"
            size="sm"
            onclick={handleSendMagicLink}
            disabled={resendCooldown > 0 || isSending}
          >
            {#if isSending}
              <Loader2 class="size-4 mr-2 animate-spin" />
              Sending...
            {:else if resendCooldown > 0}
              Resend in {resendCooldown}s
            {:else}
              Resend Email
            {/if}
          </Button>
        </div>
      {/if}

      <Button
        variant="ghost"
        size="sm"
        class="w-full"
        onclick={() => { verificationSent = false; stopMagicLinkDetection(); }}
      >
        Change Email
      </Button>
    </div>
  {/if}
</div>
