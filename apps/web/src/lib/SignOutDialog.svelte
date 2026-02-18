<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Loader2, LogOut } from "@lucide/svelte";
  import { logout } from "$lib/auth";
  import { clearAllLocalData } from "$lib/settings/clearData";
  import { getDeviceId } from "$lib/device/deviceId";

  interface Props {
    open: boolean;
  }

  let { open = $bindable() }: Props = $props();

  let isClearingData = $state(false);

  async function handleKeepLocalData() {
    open = false;
    await logout();
  }

  async function handleClearAndSignOut() {
    isClearingData = true;
    try {
      // Delete this device from the server while credentials are still valid
      const token = localStorage.getItem('diaryx_auth_token');
      const deviceId = getDeviceId();
      const serverUrl = localStorage.getItem('diaryx_sync_server_url');
      if (token && deviceId && serverUrl) {
        try {
          await fetch(`${serverUrl}/auth/devices/${deviceId}`, {
            method: 'DELETE',
            headers: { Authorization: `Bearer ${token}` },
          });
        } catch {
          // Best-effort
        }
      }

      await logout();
      await clearAllLocalData();
    } catch {
      // clearAllLocalData reloads the page on success;
      // if we get here, something failed before the reload
      isClearingData = false;
    }
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title class="flex items-center gap-2">
        <LogOut class="size-5" />
        Sign Out
      </Dialog.Title>
      <Dialog.Description>
        Would you like to clear local data? This is recommended if you're switching accounts.
      </Dialog.Description>
    </Dialog.Header>

    <p class="text-xs text-muted-foreground">
      Clearing removes all workspace files, settings, and cached data from your browser.
    </p>

    <Dialog.Footer class="gap-2 sm:gap-0">
      <Button variant="outline" onclick={() => (open = false)}>
        Cancel
      </Button>
      <Button variant="outline" onclick={handleKeepLocalData}>
        Keep Local Data
      </Button>
      <Button
        variant="destructive"
        onclick={handleClearAndSignOut}
        disabled={isClearingData}
      >
        {#if isClearingData}
          <Loader2 class="size-4 mr-2 animate-spin" />
          Clearing...
        {:else}
          Clear & Sign Out
        {/if}
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
