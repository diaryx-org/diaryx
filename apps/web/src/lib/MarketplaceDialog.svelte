<script lang="ts">
  /**
   * MarketplaceDialog - Marketplace surface for themes, typography, plugins, and bundles.
   *
   * Renders as a Drawer on mobile and Dialog on desktop.
   */
  import * as Dialog from "$lib/components/ui/dialog";
  import * as Drawer from "$lib/components/ui/drawer";
  import { Store } from "@lucide/svelte";
  import { getMobileState } from "./hooks/useMobile.svelte";
  import MarketplaceSidebar from "../views/marketplace/MarketplaceSidebar.svelte";

  interface Props {
    open?: boolean;
  }

  let {
    open = $bindable(),
  }: Props = $props();

  const mobileState = getMobileState();
</script>

{#if mobileState.isMobile}
  <Drawer.Root bind:open>
    <Drawer.Content>
      <div class="mx-auto w-full max-w-md h-[70vh] flex flex-col">
        <Drawer.Header class="shrink-0">
          <Drawer.Title class="flex items-center gap-2">
            <Store class="size-5" />
            Marketplace
          </Drawer.Title>
          <Drawer.Description>
            Browse themes, typography, plugins, and bundles.
          </Drawer.Description>
        </Drawer.Header>
        <div class="px-2 pb-[calc(env(safe-area-inset-bottom)+0.5rem)] min-h-0 flex-1">
          <MarketplaceSidebar />
        </div>
      </div>
    </Drawer.Content>
  </Drawer.Root>
{:else}
  <Dialog.Root bind:open>
    <Dialog.Content class="flex h-[680px] max-h-[calc(100dvh-2rem)] w-[calc(100%-2rem)] max-w-[780px] flex-col overflow-hidden">
      <Dialog.Header class="shrink-0 pr-8">
        <Dialog.Title class="flex items-center gap-2">
          <Store class="size-5" />
          Marketplace
        </Dialog.Title>
        <Dialog.Description>
          Browse themes, typography, plugins, and bundles.
        </Dialog.Description>
      </Dialog.Header>
      <div class="flex-1 min-h-0">
        <MarketplaceSidebar />
      </div>
    </Dialog.Content>
  </Dialog.Root>
{/if}
