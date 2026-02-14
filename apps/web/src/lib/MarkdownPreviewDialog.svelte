<script lang="ts">
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Copy, Check, FileText } from "@lucide/svelte";
  import { toast } from "svelte-sonner";
  import yaml from "js-yaml";

  interface Props {
    open: boolean;
    body: string;
    frontmatter: Record<string, unknown>;
    onOpenChange: (open: boolean) => void;
  }

  let { open, body, frontmatter, onOpenChange }: Props = $props();

  let copiedContent = $state(false);
  let copiedFile = $state(false);

  const hasFrontmatter = $derived(Object.keys(frontmatter).length > 0);

  const frontmatterYaml = $derived(
    hasFrontmatter ? yaml.dump(frontmatter, { lineWidth: -1 }).trimEnd() : ""
  );

  const fullFile = $derived(
    hasFrontmatter ? `---\n${frontmatterYaml}\n---\n\n${body}` : body
  );

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
      await navigator.clipboard.writeText(fullFile);
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
        The markdown output for this entry.
      </Dialog.Description>
    </Dialog.Header>
    <div class="flex-1 overflow-auto min-h-0">
      <pre
        class="text-sm font-mono whitespace-pre-wrap break-words bg-muted p-4 rounded-md border"
      >{fullFile}</pre>
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
