<script lang="ts">
  /**
   * AccountSettings - Account management settings
   *
   * Features:
   * - Inline sign-in (email + magic link) — does NOT enable sync
   * - Account info (email, devices)
   * - "Set Up Sync" button when signed in but sync not enabled
   * - Delete server data
   * - Logout
   */
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Separator } from "$lib/components/ui/separator";
  import * as Dialog from "$lib/components/ui/dialog";
  import {
    User,
    Loader2,
    LogOut,
    Smartphone,
    Trash2,
    RefreshCw,
    AlertCircle,
    AlertTriangle,
    Server,
    Pencil,
    Check,
    X,
    Fingerprint,
    Plus,
  } from "@lucide/svelte";
  import SignOutDialog from "$lib/SignOutDialog.svelte";
  import SignInForm from "$lib/components/SignInForm.svelte";
  import { isTauri } from "$lib/backend/interface";
  import {
    getAuthState,
    logout,
    deleteDevice,
    renameDevice,
    deleteAccount,
    refreshUserInfo,
    initAuth,
    type PasskeyListItem,
  } from "$lib/auth";
  import {
    registerPasskey,
    listPasskeys,
    deletePasskey,
  } from "$lib/auth/authStore.svelte";
  import { isPasskeySupported } from "$lib/auth/webauthnUtils";
  import { collaborationStore } from "@/models/stores/collaborationStore.svelte";
  import { onMount } from "svelte";

  interface Props {
    /** Callback to open the sync setup wizard */
    onAddWorkspace?: () => void;
  }

  let { onAddWorkspace }: Props = $props();

  // Auth state
  let authState = $derived(getAuthState());
  let syncEnabled = $derived(collaborationStore.collaborationEnabled);

  // Account management state
  let isLoggingOut = $state(false);
  let isDeleting = $state(false);
  let showDeleteConfirm = $state(false);
  let showSignOutDialog = $state(false);
  let error = $state<string | null>(null);


  // Device rename state
  let renamingDeviceId = $state<string | null>(null);
  let renameValue = $state("");

  // Passkey state
  let passkeys = $state<PasskeyListItem[]>([]);
  let passkeySupported = $state(false);
  let isAddingPasskey = $state(false);
  let passkeyName = $state("");
  let passkeyError = $state<string | null>(null);

  async function loadPasskeys() {
    if (authState.isAuthenticated) {
      passkeys = await listPasskeys();
    }
  }

  async function handleAddPasskey() {
    const name = passkeyName.trim() || "My Passkey";
    isAddingPasskey = true;
    passkeyError = null;
    try {
      await registerPasskey(name);
      passkeyName = "";
      await loadPasskeys();
    } catch (e) {
      passkeyError = e instanceof Error ? e.message : "Failed to add passkey";
    } finally {
      isAddingPasskey = false;
    }
  }

  async function handleDeletePasskey(id: string) {
    try {
      await deletePasskey(id);
      await loadPasskeys();
    } catch (e) {
      passkeyError = e instanceof Error ? e.message : "Failed to delete passkey";
    }
  }

  onMount(() => {
    initAuth();
    isPasskeySupported().then((v) => { passkeySupported = v; });
    loadPasskeys();
  });

  // ── Account management handlers ──

  async function handleLogout() {
    if (isTauri()) {
      isLoggingOut = true;
      try {
        await logout();
      } finally {
        isLoggingOut = false;
      }
    } else {
      showSignOutDialog = true;
    }
  }

  async function handleDeleteDevice(deviceId: string) {
    try { await deleteDevice(deviceId); }
    catch (e) { error = e instanceof Error ? e.message : "Failed to delete device"; }
  }

  async function handleRefresh() {
    try { await refreshUserInfo(); }
    catch (e) { error = e instanceof Error ? e.message : "Failed to refresh"; }
  }

  function startRenameDevice(deviceId: string, currentName: string) {
    renamingDeviceId = deviceId;
    renameValue = currentName;
  }

  function cancelRenameDevice() {
    renamingDeviceId = null;
    renameValue = "";
  }

  async function confirmRenameDevice() {
    if (!renamingDeviceId || !renameValue.trim()) return;
    try {
      await renameDevice(renamingDeviceId, renameValue.trim());
      renamingDeviceId = null;
      renameValue = "";
    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to rename device";
    }
  }

  async function handleDeleteAccount() {
    isDeleting = true;
    error = null;
    try { await deleteAccount(); showDeleteConfirm = false; }
    catch (e) { error = e instanceof Error ? e.message : "Failed to delete account"; }
    finally { isDeleting = false; }
  }
</script>

<div class="space-y-4">
  <!-- Header -->
  <h3 class="font-medium flex items-center gap-2">
    <User class="size-4" />
    Account
  </h3>

  <!-- Error Message -->
  {#if error}
    <div class="flex items-center gap-2 text-destructive text-sm p-2 bg-destructive/10 rounded-md">
      <AlertCircle class="size-4 shrink-0" />
      <span>{error}</span>
    </div>
  {/if}

  {#if authState.isAuthenticated && authState.user}
    <!-- Logged In State -->
    <div class="space-y-4">
      <!-- Email -->
      <div class="flex items-center justify-between">
        <div class="flex items-center gap-2">
          <User class="size-4 text-muted-foreground" />
          <span class="text-sm font-medium">{authState.user.email}</span>
        </div>
        <Button variant="ghost" size="sm" onclick={handleRefresh}>
          <RefreshCw class="size-4" />
        </Button>
      </div>

      <!-- Sync Setup Prompt (when signed in but sync not enabled) -->
      {#if !syncEnabled && onAddWorkspace}
        <div class="space-y-2 p-3 rounded-md bg-primary/5 border border-primary/20">
          <p class="text-xs text-muted-foreground">
            Signed in. Set up sync to access your notes across devices. One synced workspace is free.
          </p>
          <Button
            variant="default"
            size="sm"
            class="w-full"
            onclick={onAddWorkspace}
          >
            <Server class="size-4 mr-2" />
            Set Up Sync
          </Button>
        </div>
      {/if}

      <Separator />

      <!-- Devices -->
      {#if authState.devices.length > 0}
        <div class="space-y-2">
          <Label class="text-xs text-muted-foreground">Your Devices</Label>
          <div class="space-y-1">
            {#each authState.devices as device}
              <div class="flex items-center justify-between text-sm p-2 bg-muted/50 rounded-md">
                {#if renamingDeviceId === device.id}
                  <div class="flex items-center gap-1 flex-1 mr-2">
                    <Smartphone class="size-4 text-muted-foreground shrink-0" />
                    <Input
                      type="text"
                      bind:value={renameValue}
                      class="h-7 text-sm"
                      onkeydown={(e) => {
                        if (e.key === "Enter") confirmRenameDevice();
                        if (e.key === "Escape") cancelRenameDevice();
                      }}
                    />
                  </div>
                  <div class="flex items-center gap-0.5">
                    <Button
                      variant="ghost"
                      size="sm"
                      class="h-7 w-7 p-0"
                      onclick={confirmRenameDevice}
                    >
                      <Check class="size-3 text-primary" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="sm"
                      class="h-7 w-7 p-0"
                      onclick={cancelRenameDevice}
                    >
                      <X class="size-3 text-muted-foreground" />
                    </Button>
                  </div>
                {:else}
                  <div class="flex items-center gap-2">
                    <Smartphone class="size-4 text-muted-foreground" />
                    <span>{device.name || "Unknown Device"}</span>
                  </div>
                  <div class="flex items-center gap-0.5">
                    <Button
                      variant="ghost"
                      size="sm"
                      class="h-7 w-7 p-0"
                      onclick={() => startRenameDevice(device.id, device.name || "")}
                    >
                      <Pencil class="size-3 text-muted-foreground" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="sm"
                      class="h-7 w-7 p-0"
                      onclick={() => handleDeleteDevice(device.id)}
                    >
                      <Trash2 class="size-3 text-muted-foreground" />
                    </Button>
                  </div>
                {/if}
              </div>
            {/each}
          </div>
        </div>

        <Separator />
      {/if}

      <!-- Passkeys -->
      {#if passkeySupported}
        <div class="space-y-2">
          <Label class="text-xs text-muted-foreground">Passkeys</Label>

          {#if passkeys.length > 0}
            <div class="space-y-1">
              {#each passkeys as pk}
                <div class="flex items-center justify-between text-sm p-2 bg-muted/50 rounded-md">
                  <div class="flex items-center gap-2">
                    <Fingerprint class="size-4 text-muted-foreground" />
                    <span>{pk.name}</span>
                  </div>
                  <Button
                    variant="ghost"
                    size="sm"
                    class="h-7 w-7 p-0"
                    onclick={() => handleDeletePasskey(pk.id)}
                  >
                    <Trash2 class="size-3 text-muted-foreground" />
                  </Button>
                </div>
              {/each}
            </div>
          {:else}
            <p class="text-xs text-muted-foreground">
              No passkeys registered. Add one for faster sign-in.
            </p>
          {/if}

          {#if passkeyError}
            <p class="text-xs text-destructive">{passkeyError}</p>
          {/if}

          <div class="flex items-center gap-2">
            <Input
              type="text"
              placeholder="Passkey name"
              bind:value={passkeyName}
              class="h-8 text-sm flex-1"
              onkeydown={(e) => { if (e.key === "Enter") handleAddPasskey(); }}
            />
            <Button
              variant="outline"
              size="sm"
              class="h-8"
              onclick={handleAddPasskey}
              disabled={isAddingPasskey}
            >
              {#if isAddingPasskey}
                <Loader2 class="size-3.5 mr-1 animate-spin" />
              {:else}
                <Plus class="size-3.5 mr-1" />
              {/if}
              Add
            </Button>
          </div>
        </div>

        <Separator />
      {/if}

      <!-- Account Actions -->
      <div class="space-y-2">
        <Label class="text-xs text-muted-foreground">Actions</Label>

        <Button
          variant="outline"
          size="sm"
          class="w-full justify-start"
          onclick={handleLogout}
          disabled={isLoggingOut}
        >
          {#if isLoggingOut}
            <Loader2 class="size-4 mr-2 animate-spin" />
          {:else}
            <LogOut class="size-4 mr-2" />
          {/if}
          Sign Out
        </Button>

        <Button
          variant="destructive"
          size="sm"
          class="w-full justify-start"
          onclick={() => (showDeleteConfirm = true)}
        >
          <Trash2 class="size-4 mr-2" />
          Delete All Server Data
        </Button>
      </div>

      <p class="text-xs text-muted-foreground">
        Deleting server data will remove all synced data from our servers but keep your local files.
      </p>
    </div>
  {:else}
    <!-- Not Authenticated: Inline sign-in form -->
    <SignInForm
      onAuthenticated={() => {
        if (getAuthState().workspaces.length > 0 && !syncEnabled) {
          onAddWorkspace?.();
        }
      }}
    />
  {/if}
</div>

<!-- Delete Confirmation Dialog -->
<Dialog.Root bind:open={showDeleteConfirm}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title class="flex items-center gap-2 text-destructive">
        <AlertTriangle class="size-5" />
        Delete All Server Data
      </Dialog.Title>
      <Dialog.Description>
        This will permanently delete all your data from our sync servers, including:
      </Dialog.Description>
    </Dialog.Header>

    <ul class="list-disc list-inside text-sm text-muted-foreground space-y-1 py-2">
      <li>Your synced workspace data</li>
      <li>All linked devices</li>
      <li>Your account information</li>
    </ul>

    <p class="text-sm font-medium">
      Your local files will NOT be deleted.
    </p>

    <Dialog.Footer class="gap-2 sm:gap-0">
      <Button variant="outline" onclick={() => (showDeleteConfirm = false)}>
        Cancel
      </Button>
      <Button
        variant="destructive"
        onclick={handleDeleteAccount}
        disabled={isDeleting}
      >
        {#if isDeleting}
          <Loader2 class="size-4 mr-2 animate-spin" />
        {/if}
        Delete Everything
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<SignOutDialog bind:open={showSignOutDialog} />
