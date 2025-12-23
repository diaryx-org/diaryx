<script lang="ts">
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Settings } from "@lucide/svelte";

  interface Props {
    open: boolean;
  }

  let { open = $bindable() }: Props = $props();
</script>

<Dialog.Root bind:open>
  <Dialog.Content class="sm:max-w-[425px]">
    <Dialog.Header>
      <Dialog.Title class="flex items-center gap-2">
        <Settings class="size-5" />
        Settings
      </Dialog.Title>
      <Dialog.Description>
        Configure your workspace preferences.
      </Dialog.Description>
    </Dialog.Header>
    
    <div class="py-8 text-center">
      <p class="text-muted-foreground">Settings coming soon...</p>
    </div>
    
    <Dialog.Footer>
      <Button variant="outline" onclick={() => open = false}>
        Close
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

