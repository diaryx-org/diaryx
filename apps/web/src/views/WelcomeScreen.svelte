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

  async function handleCreate() {
    const name = workspaceName.trim() || "My Journal";
    isCreating = true;
    error = null;

    try {
      // Create the workspace in the local registry
      const ws = createLocalWorkspace(name);
      setCurrentWorkspaceId(ws.id);

      if (includeGuide) {
        // Initialize backend with the new workspace and create root index
        const backend = await getBackend(ws.id, ws.name, getWorkspaceStorageType(ws.id));
        const api = createApi(backend);
        await api.createWorkspace(".", name);
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
