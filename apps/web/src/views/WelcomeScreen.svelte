<script lang="ts">
  /**
   * WelcomeScreen — full-screen first-run experience shown when no workspaces exist.
   *
   * Lets the user create their first workspace with:
   * - Workspace name input (default: "My Journal")
   * - Checkbox to include a getting-started guide (root index)
   * - Sign-in link for returning users
   */
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Checkbox } from "$lib/components/ui/checkbox";
  import { Loader2 } from "@lucide/svelte";
  import {
    createLocalWorkspace,
    setCurrentWorkspaceId,
    getWorkspaceStorageType,
  } from "$lib/storage/localWorkspaceRegistry.svelte";
  import { getBackend } from "$lib/backend";
  import { createApi } from "$lib/backend/api";

  interface Props {
    onWorkspaceCreated: (id: string, name: string) => void;
    onSignIn: () => void;
  }

  let { onWorkspaceCreated, onSignIn }: Props = $props();

  let workspaceName = $state("My Journal");
  let includeGuide = $state(true);
  let isCreating = $state(false);
  let error = $state<string | null>(null);

  function getGuideContent(name: string): string {
    return `---
title: ${name}
description: A diaryx workspace
contents: []
---

# Welcome to Diaryx

Diaryx is a workspace for your notes, journals, and documents — organized into a navigable hierarchy. Here's a quick tour to get you started.

## Creating entries

- Press **Cmd/Ctrl + N** or click the **+** button in the left sidebar to create a new entry.
- Use the **command palette** (**Cmd/Ctrl + K**) and type "New Entry" or "Daily Entry."
- Right-click any entry in the sidebar to create a **child entry** underneath it.

## Writing and formatting

Start typing on any blank line and click the **+** button that appears to insert headings, lists, tables, code blocks, and more. Select text to reveal a floating toolbar for **bold**, *italic*, highlights, links, and other inline styles.

Drag and drop images or files directly into the editor to attach them.

## Organizing your workspace

Entries are organized in a **parent-child hierarchy** — like folders and files. Each parent entry lists its children, and you can nest entries as deep as you like. Drag entries in the sidebar to reorder or rearrange them.

## The command palette

**Cmd/Ctrl + K** opens the command palette — a searchable hub for everything: open entries, create new ones, validate your workspace, export, change settings, and more.

## Keyboard shortcuts

| Shortcut | Action |
|---|---|
| Cmd/Ctrl + K | Command palette |
| Cmd/Ctrl + N | New entry |
| Cmd/Ctrl + S | Save |
| Cmd/Ctrl + [ | Toggle left sidebar |
| Cmd/Ctrl + ] | Toggle right sidebar |
| Cmd/Ctrl + F | Find in file |

## Templates

Create reusable templates for common entry types (meeting notes, daily journals, etc.) in **Settings → Templates**. Templates support variables like \`{{title}}\`, \`{{date}}\`, and \`{{time}}\`.

## Properties and metadata

Click the properties button (or press **Cmd/Ctrl + ]**) to open the right sidebar. From there you can edit frontmatter fields like title, date, and tags, manage attachments, view version history, and set up sharing.

## Sync and collaboration

Sign in to sync your workspace across devices and collaborate in real time. Create a **share session** to let others edit alongside you with live cursors and conflict-free merging.

## Next steps

- Delete this guide once you're comfortable — it's just a regular entry.
- Explore **Settings** (via the command palette) to customize templates, display preferences, and more.
- Use **Validate Workspace** from the command palette to check for any structural issues.

Happy writing!
`;
  }

  async function handleCreate() {
    const name = workspaceName.trim() || "My Journal";
    isCreating = true;
    error = null;

    try {
      // Create the workspace in the local registry
      const ws = createLocalWorkspace(name);
      setCurrentWorkspaceId(ws.id);

      if (includeGuide) {
        // Initialize backend with the new workspace and create root index with guide content
        const backend = await getBackend(ws.id, ws.name, getWorkspaceStorageType(ws.id));
        const api = createApi(backend);
        await api.createWorkspace(".", name);
        await api.saveEntry("./README.md", getGuideContent(name));
      }

      onWorkspaceCreated(ws.id, name);
    } catch (e) {
      console.error("[WelcomeScreen] Failed to create workspace:", e);
      error = e instanceof Error ? e.message : "Failed to create workspace";
      isCreating = false;
    }
  }
</script>

<div class="flex items-center justify-center min-h-dvh bg-background px-4">
  <div class="w-full max-w-sm space-y-6">
    <div class="text-center space-y-2">
      <h1 class="text-3xl font-bold tracking-tight text-foreground">
        Welcome to Diaryx
      </h1>
      <p class="text-muted-foreground text-sm">
        A workspace organizes your notes, journals, and documents into a navigable hierarchy.
      </p>
    </div>

    <div class="space-y-4">
      <div class="space-y-2">
        <Label for="workspace-name" class="text-sm">Workspace Name</Label>
        <Input
          id="workspace-name"
          type="text"
          bind:value={workspaceName}
          placeholder="My Journal"
          disabled={isCreating}
          onkeydown={(e) => e.key === "Enter" && handleCreate()}
        />
      </div>

      <div class="flex items-center gap-2">
        <Checkbox
          id="include-guide"
          checked={includeGuide}
          onCheckedChange={(checked) => { includeGuide = checked === true; }}
          disabled={isCreating}
        />
        <Label for="include-guide" class="text-sm text-muted-foreground cursor-pointer">
          Include a file with a helpful guide
        </Label>
      </div>

      {#if error}
        <p class="text-sm text-destructive">{error}</p>
      {/if}

      <Button
        class="w-full"
        onclick={handleCreate}
        disabled={isCreating}
      >
        {#if isCreating}
          <Loader2 class="size-4 mr-2 animate-spin" />
          Creating...
        {:else}
          Create Workspace
        {/if}
      </Button>
    </div>

    <p class="text-center text-xs text-muted-foreground">
      Already have an account?
      <button
        type="button"
        class="text-primary hover:underline"
        onclick={onSignIn}
        disabled={isCreating}
      >
        Sign in
      </button>
    </p>
  </div>
</div>
