<script lang="ts">
  import * as Dialog from "$lib/components/ui/dialog";
  import { Globe } from "@lucide/svelte";
  import type { Api } from "$lib/backend/api";
  import PublishingPanel from "./PublishingPanel.svelte";

  interface Props {
    open: boolean;
    api: Api | null;
    onOpenChange?: (open: boolean) => void;
  }

  let { open = $bindable(), api, onOpenChange }: Props = $props();
</script>

<Dialog.Root bind:open {onOpenChange}>
  <Dialog.Content
    class="sm:max-w-[480px] max-h-[85vh] flex flex-col gap-0 p-0"
    data-testid="publish-dialog"
  >
    <Dialog.Header class="px-4 pt-4 pb-2 border-b">
      <Dialog.Title class="flex items-center gap-2">
        <Globe class="size-5" />
        Publish
      </Dialog.Title>
      <Dialog.Description>
        Publish your workspace audiences to your site.
      </Dialog.Description>
    </Dialog.Header>

    <div class="flex-1 overflow-y-auto">
      <PublishingPanel {api} />
    </div>
  </Dialog.Content>
</Dialog.Root>
