<script lang="ts">
  import type { Api } from "$lib/backend/api";
  import { Button } from "$lib/components/ui/button";
  import ExportDialog from "$lib/ExportDialog.svelte";
  import PublishingPanel from "$lib/share/PublishingPanel.svelte";
  import { Download, Globe } from "@lucide/svelte";

  interface Props {
    api: Api | null;
    rootPath: string;
    onAddWorkspace?: () => void;
  }

  let { api, rootPath, onAddWorkspace }: Props = $props();

  let exportDialogOpen = $state(false);

  const canExport = $derived(!!api && !!rootPath);
</script>

<div class="px-3 py-3 space-y-4">
  <div class="rounded-md border border-border bg-card p-3 space-y-3">
    <div class="flex items-start justify-between gap-3">
      <div>
        <h3 class="text-sm font-medium flex items-center gap-2">
          <Download class="size-4" />
          Export
        </h3>
        <p class="text-xs text-muted-foreground mt-1">
          Export this workspace to markdown, HTML, or converter-based formats.
        </p>
      </div>
    </div>

    <Button
      variant="outline"
      size="sm"
      onclick={() => (exportDialogOpen = true)}
      disabled={!canExport}
    >
      <Download class="size-4 mr-1.5" />
      Export Workspace
    </Button>
  </div>

  <div class="rounded-md border border-border bg-card p-0 overflow-hidden">
    <div class="px-3 pt-3 pb-1 border-b border-border/60">
      <h3 class="text-sm font-medium flex items-center gap-2">
        <Globe class="size-4" />
        Site Publishing
      </h3>
    </div>
    <PublishingPanel {onAddWorkspace} {api} />
  </div>
</div>

<ExportDialog
  open={exportDialogOpen}
  rootPath={rootPath}
  {api}
  onOpenChange={(value) => (exportDialogOpen = value)}
/>
