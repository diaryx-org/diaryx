<script lang="ts">
  /**
   * LinkSettings - Link conversion actions for the workspace
   *
   * Provides a conversion action to rewrite existing links using the current
   * workspace link format.
   */
  import { Button } from "$lib/components/ui/button";
  import { Link, RefreshCw, AlertCircle, Check } from "@lucide/svelte";
  import { getLinkFormatStore } from "../stores/linkFormatStore.svelte";

  interface Props {
    workspaceRootIndex?: string | null;
  }

  let { workspaceRootIndex = null }: Props = $props();

  const linkFormatStore = getLinkFormatStore();

  // Conversion state
  let isConverting = $state(false);
  let conversionResult = $state<{
    files_modified: number;
    links_converted: number;
  } | null>(null);
  let showResult = $state(false);

  // Load the link format when workspace root index changes
  $effect(() => {
    if (workspaceRootIndex) {
      linkFormatStore.load(workspaceRootIndex);
    }
  });

  async function handleConvertLinks() {
    isConverting = true;
    conversionResult = null;
    showResult = false;

    try {
      const result = await linkFormatStore.convertLinks(linkFormatStore.format, {
        dryRun: false,
      });

      if (result) {
        conversionResult = {
          files_modified: result.files_modified,
          links_converted: result.links_converted,
        };
        showResult = true;

        // Hide result after 5 seconds
        setTimeout(() => {
          showResult = false;
        }, 5000);
      }
    } finally {
      isConverting = false;
    }
  }
</script>

<div class="space-y-4">
  <div class="space-y-3">
    <h3 class="font-medium flex items-center gap-2">
      <Link class="size-4" />
      Link Format
    </h3>

    <p class="text-xs text-muted-foreground px-1">
      Convert links in <code class="bg-muted px-1 rounded">part_of</code> and
      <code class="bg-muted px-1 rounded">contents</code> and
      <code class="bg-muted px-1 rounded">attachments</code> properties to your current workspace format.
    </p>

    <div class="space-y-3 px-1">
      <div class="space-y-1">
        <p class="text-xs text-muted-foreground">
          Current link format:
          <code class="bg-muted px-1 rounded ml-1">{linkFormatStore.getFormatLabel(linkFormatStore.format)}</code>
        </p>
        <p class="text-xs text-muted-foreground">
          {linkFormatStore.getFormatDescription(linkFormatStore.format)}
        </p>
      </div>

      {#if linkFormatStore.error}
        <div class="flex items-center gap-2 text-xs text-destructive">
          <AlertCircle class="size-3" />
          <span>{linkFormatStore.error}</span>
        </div>
      {/if}

      <div class="pt-2 border-t space-y-2">
        <p class="text-xs text-muted-foreground">
          Convert all existing links in your workspace to the current workspace format.
          This will update <code class="bg-muted px-1 rounded">part_of</code> and
          <code class="bg-muted px-1 rounded">contents</code> and
          <code class="bg-muted px-1 rounded">attachments</code> in all files.
        </p>

        <Button
          variant="outline"
          size="sm"
          class="w-full"
          onclick={handleConvertLinks}
          disabled={isConverting || linkFormatStore.loading || !workspaceRootIndex}
        >
          {#if isConverting}
            <RefreshCw class="size-4 mr-2 animate-spin" />
            Converting...
          {:else}
            <RefreshCw class="size-4 mr-2" />
            Convert All Links
          {/if}
        </Button>

        {#if showResult && conversionResult}
          <div class="flex items-center gap-2 text-xs text-green-600 dark:text-green-400">
            <Check class="size-3" />
            <span>
              Converted {conversionResult.links_converted} link{conversionResult.links_converted !== 1 ? 's' : ''}
              in {conversionResult.files_modified} file{conversionResult.files_modified !== 1 ? 's' : ''}.
            </span>
          </div>
        {/if}
      </div>
    </div>
  </div>
</div>
