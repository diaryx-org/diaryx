<script lang="ts">
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Copy, Check, FileText } from "@lucide/svelte";
  import { toast } from "svelte-sonner";

  interface Props {
    open: boolean;
    /** The raw file contents, exactly as stored on disk. */
    raw: string;
    onOpenChange: (open: boolean) => void;
  }

  let { open, raw, onOpenChange }: Props = $props();

  let copiedContent = $state(false);
  let copiedFile = $state(false);

  // The body is everything after the closing frontmatter `---` fence. We split
  // here only to support the "Copy Content" button; the displayed source is
  // always the verbatim raw file.
  const body = $derived.by(() => {
    const match = raw.match(/^---\r?\n[\s\S]*?\r?\n---\r?\n?/);
    return match ? raw.slice(match[0].length) : raw;
  });

  async function copyContent() {
    try {
      await navigator.clipboard.writeText(body);
      copiedContent = true;
      setTimeout(() => (copiedContent = false), 2000);
    } catch {
      toast.error("Failed to copy");
    }
  }

  async function copyFile() {
    try {
      await navigator.clipboard.writeText(raw);
      copiedFile = true;
      setTimeout(() => (copiedFile = false), 2000);
    } catch {
      toast.error("Failed to copy");
    }
  }
</script>

<Dialog.Root {open} {onOpenChange}>
  <Dialog.Content class="max-w-2xl max-h-[80vh] flex flex-col">
    <Dialog.Header>
      <Dialog.Title>Markdown Source</Dialog.Title>
      <Dialog.Description>
        The raw file contents for this entry.
      </Dialog.Description>
    </Dialog.Header>
    <div class="flex-1 overflow-auto min-h-0">
      <pre
        class="text-sm font-mono whitespace-pre-wrap break-words bg-muted p-4 rounded-md border"
      >{raw}</pre>
    </div>
    <Dialog.Footer class="gap-2 sm:gap-0">
      <Button variant="outline" size="sm" onclick={copyContent}>
        {#if copiedContent}
          <Check class="mr-2 size-4" />
          Copied
        {:else}
          <Copy class="mr-2 size-4" />
          Copy Content
        {/if}
      </Button>
      <Button variant="outline" size="sm" onclick={copyFile}>
        {#if copiedFile}
          <Check class="mr-2 size-4" />
          Copied
        {:else}
          <FileText class="mr-2 size-4" />
          Copy File
        {/if}
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
