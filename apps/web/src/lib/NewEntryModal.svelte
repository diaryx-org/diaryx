<script lang="ts">
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";

  interface Props {
    onSave: (path: string, title: string) => void;
    onCancel: () => void;
  }

  let { onSave, onCancel }: Props = $props();

  let path = $state("");
  let title = $state("");
  let open = $state(true);

  function normalizeEntryPath(input: string): string {
    const raw = input.trim();
    if (!raw) return "";

    // If the user only typed a filename (no folder), default to workspace root.
    // (We keep it simple: no leading "./", no absolute paths.)
    const hasFolder = raw.includes("/");
    if (!hasFolder) return raw;

    // Otherwise, use what they typed.
    return raw;
  }

  function handleSave() {
    const normalizedPath = normalizeEntryPath(path);
    if (normalizedPath) {
      onSave(normalizedPath, title);
      open = false;
    }
  }

  function handleOpenChange(isOpen: boolean) {
    open = isOpen;
    if (!isOpen) {
      onCancel();
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && normalizeEntryPath(path)) {
      e.preventDefault();
      handleSave();
    }
  }
</script>

<Dialog.Root {open} onOpenChange={handleOpenChange}>
  <Dialog.Content class="sm:max-w-[425px]">
    <Dialog.Header>
      <Dialog.Title>New Entry</Dialog.Title>
      <Dialog.Description>
        Create a new journal entry. Enter a filename (it will be created in the
        workspace root) or a full relative path, plus an optional title.
      </Dialog.Description>
    </Dialog.Header>

    <div class="grid gap-4 py-4">
      <div class="grid gap-2">
        <Label for="entry-path">Path</Label>
        <Input
          id="entry-path"
          bind:value={path}
          placeholder="e.g., 2025-01-15.md (workspace root) or journal/2025-01-15.md"
          onkeydown={handleKeydown}
        />
        <p class="text-xs text-muted-foreground">
          If you enter only a filename, it will be created in the workspace root
          folder. Paths should end with .md
        </p>
      </div>

      <div class="grid gap-2">
        <Label for="entry-title">Title (Optional)</Label>
        <Input
          id="entry-title"
          bind:value={title}
          placeholder="My Journal Entry"
          onkeydown={handleKeydown}
        />
      </div>
    </div>

    <Dialog.Footer>
      <Button variant="outline" onclick={() => handleOpenChange(false)}>
        Cancel
      </Button>
      <Button onclick={handleSave} disabled={!normalizeEntryPath(path)}
        >Create</Button
      >
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
