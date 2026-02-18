<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Separator } from "$lib/components/ui/separator";
  import {
    Loader2,
    LogOut,
    Mail,
    Link,
    ChevronDown,
    ChevronUp,
    Server,
    Settings,
    AlertCircle,
    Fingerprint,
  } from "@lucide/svelte";
  import SignOutDialog from "$lib/SignOutDialog.svelte";
  import VerificationCodeInput from "$lib/components/VerificationCodeInput.svelte";
  import {
    getAuthState,
    logout,
    initAuth,
    setServerUrl,
    requestMagicLink,
    verifyMagicLink,
  } from "$lib/auth";
  import {
    authenticateWithPasskey,
  } from "$lib/auth/authStore.svelte";
  import { isPasskeySupported } from "$lib/auth/webauthnUtils";
  import { isTauri } from "$lib/backend/interface";
  import { collaborationStore } from "@/models/stores/collaborationStore.svelte";
  import { onMount } from "svelte";

  interface Props {
    onOpenAccountSettings?: () => void;
    onOpenSyncWizard?: () => void;
    onClose?: () => void;
  }

  let { onOpenAccountSettings, onOpenSyncWizard, onClose }: Props = $props();

  let authState = $derived(getAuthState());
  let syncEnabled = $derived(collaborationStore.collaborationEnabled);

  // Sign-in state
  let email = $state("");
  let serverUrl = $state(
    typeof window !== "undefined"
      ? localStorage.getItem("diaryx_sync_server_url") || "https://sync.diaryx.org"
      : "https://sync.diaryx.org"
  );
  let showAdvanced = $state(false);
  let verificationSent = $state(false);
  let devLink = $state<string | null>(null);
  let isSending = $state(false);
  let isValidating = $state(false);
  let isLoggingOut = $state(false);
  let showSignOutDialog = $state(false);
  let error = $state<string | null>(null);
  let resendCooldown = $state(0);
  let resendInterval: ReturnType<typeof setInterval> | null = null;
  let urlCheckInterval: ReturnType<typeof setInterval> | null = null;

  // Passkey state
  let passkeySupported = $state(false);
  let isAuthenticatingPasskey = $state(false);

  async function handlePasskeySignIn() {
    if (!(await validateServer())) return;
    isAuthenticatingPasskey = true;
    error = null;
    try {
      await authenticateWithPasskey(email.trim() || undefined);
      email = "";
      if (getAuthState().workspaces.length > 0 && !syncEnabled) {
        onClose?.();
        onOpenSyncWizard?.();
      }
    } catch (e) {
      error = e instanceof Error ? e.message : "Passkey authentication failed";
    } finally {
      isAuthenticatingPasskey = false;
    }
  }

  onMount(() => {
    initAuth();
    isPasskeySupported().then((v) => { passkeySupported = v; });
    return () => {
      stopMagicLinkDetection();
      if (resendInterval) clearInterval(resendInterval);
    };
  });

  async function validateServer(): Promise<boolean> {
    let url = serverUrl.trim();
    if (!url) { error = "Please enter a server URL"; return false; }
    if (!url.startsWith("http://") && !url.startsWith("https://")) {
      url = "https://" + url;
      serverUrl = url;
    }
    isValidating = true;
    error = null;
    try {
      const resp = await fetch(`${url}/health`, { method: "GET", signal: AbortSignal.timeout(5000) });
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
      // Auto-open sync wizard for returning users with server workspaces
      if (getAuthState().workspaces.length > 0 && !syncEnabled) {
        onClose?.();
        onOpenSyncWizard?.();
      }
    } catch (e) {
      error = e instanceof Error ? e.message : "Verification failed";
    }
  }

  async function handleLogout() {
    if (isTauri()) {
      isLoggingOut = true;
      try { await logout(); } finally { isLoggingOut = false; }
    } else {
      showSignOutDialog = true;
    }
  }
</script>

<div class="w-72 space-y-3">
  {#if error}
    <div class="flex items-center gap-2 text-destructive text-xs p-2 bg-destructive/10 rounded-md">
      <AlertCircle class="size-3.5 shrink-0" />
      <span>{error}</span>
    </div>
  {/if}

  {#if authState.isAuthenticated && authState.user}
    <!-- Authenticated state -->
    <div class="space-y-3">
      <div class="text-sm font-medium truncate">{authState.user.email}</div>

      {#if !syncEnabled && onOpenSyncWizard}
        <div class="space-y-2 p-2.5 rounded-md bg-primary/5 border border-primary/20">
          <p class="text-xs text-muted-foreground">
            Set up sync to access your notes across devices.
          </p>
          <Button
            variant="default"
            size="sm"
            class="w-full"
            onclick={() => { onClose?.(); onOpenSyncWizard?.(); }}
          >
            <Server class="size-3.5 mr-1.5" />
            Set Up Sync
          </Button>
        </div>
      {/if}

      <Separator />

      <div class="space-y-1.5">
        <Button
          variant="ghost"
          size="sm"
          class="w-full justify-start"
          onclick={() => { onClose?.(); onOpenAccountSettings?.(); }}
        >
          <Settings class="size-3.5 mr-1.5" />
          Account Settings
        </Button>
        <Button
          variant="ghost"
          size="sm"
          class="w-full justify-start text-muted-foreground"
          onclick={handleLogout}
          disabled={isLoggingOut}
        >
          {#if isLoggingOut}
            <Loader2 class="size-3.5 mr-1.5 animate-spin" />
          {:else}
            <LogOut class="size-3.5 mr-1.5" />
          {/if}
          Sign Out
        </Button>
      </div>
    </div>
  {:else}
    <!-- Not authenticated: sign-in form -->
    <div class="space-y-3">
      {#if !verificationSent}
        <p class="text-xs text-muted-foreground">
          Sign in to sync across devices and host live editing sessions.
        </p>

        <div class="space-y-1.5">
          <Label for="popover-email" class="text-xs">Email</Label>
          <Input
            id="popover-email"
            type="email"
            bind:value={email}
            placeholder="you@example.com"
            disabled={isSending || isValidating}
            onkeydown={(e) => e.key === "Enter" && handleSendMagicLink()}
            class="h-8 text-sm"
          />
        </div>

        <!-- Advanced settings -->
        <div>
          <button
            type="button"
            class="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground transition-colors"
            onclick={() => showAdvanced = !showAdvanced}
          >
            {#if showAdvanced}
              <ChevronUp class="size-3" />
            {:else}
              <ChevronDown class="size-3" />
            {/if}
            Advanced
          </button>
          {#if showAdvanced}
            <div class="space-y-1.5 mt-2">
              <Label for="popover-server-url" class="text-xs">Server URL</Label>
              <Input
                id="popover-server-url"
                type="text"
                bind:value={serverUrl}
                placeholder="https://sync.diaryx.org"
                disabled={isSending || isValidating}
                class="h-8 text-sm"
              />
            </div>
          {/if}
        </div>

        {#if passkeySupported}
          <Button
            class="w-full"
            size="sm"
            onclick={handlePasskeySignIn}
            disabled={isAuthenticatingPasskey || isSending || isValidating}
          >
            {#if isAuthenticatingPasskey}
              <Loader2 class="size-3.5 mr-1.5 animate-spin" />
              Authenticating...
            {:else}
              <Fingerprint class="size-3.5 mr-1.5" />
              Sign in with Passkey
            {/if}
          </Button>

          <div class="flex items-center gap-2">
            <div class="h-px flex-1 bg-border"></div>
            <span class="text-[11px] text-muted-foreground">or</span>
            <div class="h-px flex-1 bg-border"></div>
          </div>
        {/if}

        <Button
          class="w-full"
          size="sm"
          variant={passkeySupported ? "outline" : "default"}
          onclick={handleSendMagicLink}
          disabled={isSending || isValidating || !email.trim()}
        >
          {#if isSending || isValidating}
            <Loader2 class="size-3.5 mr-1.5 animate-spin" />
            {isValidating ? 'Connecting...' : 'Sending...'}
          {:else}
            <Mail class="size-3.5 mr-1.5" />
            Send Sign-in Link
          {/if}
        </Button>
      {:else}
        <!-- Waiting for verification -->
        <div class="space-y-3">
          {#if devLink}
            <div class="space-y-2 p-2.5 bg-amber-500/10 rounded-md">
              <p class="text-xs text-amber-700 dark:text-amber-400 font-medium">
                Dev mode: Email not configured
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
            <div class="text-center space-y-1.5 py-2">
              <Mail class="size-8 mx-auto text-muted-foreground" />
              <p class="text-xs font-medium">
                Check your email at <span class="text-primary">{email}</span>
              </p>
              <p class="text-[11px] text-muted-foreground">
                Click the link in your email to continue.
              </p>
            </div>

            <VerificationCodeInput
              {email}
              onVerified={() => {
                verificationSent = false;
                stopMagicLinkDetection();
                email = "";
                if (getAuthState().workspaces.length > 0 && !syncEnabled) {
                  onClose?.();
                  onOpenSyncWizard?.();
                }
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
                  <Loader2 class="size-3.5 mr-1.5 animate-spin" />
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
  {/if}
</div>

<SignOutDialog bind:open={showSignOutDialog} />
