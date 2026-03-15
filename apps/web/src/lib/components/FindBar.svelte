<script lang="ts">
  import { Input } from "$lib/components/ui/input";
  import { Button } from "$lib/components/ui/button";
  import { getMobileState } from "$lib/hooks/useMobile.svelte";
  import { ChevronUp, ChevronDown, X } from "@lucide/svelte";
  import { tick } from "svelte";

  interface Props {
    open: boolean;
    editorRef: any;
  }

  let {
    open = $bindable(),
    editorRef,
  }: Props = $props();

  const mobileState = getMobileState();

  let searchTerm = $state("");
  let inputEl: HTMLInputElement | null = $state(null);
  let resultCount = $state(0);
  let currentIndex = $state(-1);

  // Focus input when opened, clear when closed
  $effect(() => {
    if (open) {
      tick().then(() => inputEl?.focus());
    } else {
      searchTerm = "";
      editorRef?.getEditor?.()?.commands?.clearSearch?.();
      resultCount = 0;
      currentIndex = -1;
    }
  });

  function getEditor() {
    return editorRef?.getEditor?.();
  }

  function handleSearch() {
    const editor = getEditor();
    if (!editor || !searchTerm) {
      resultCount = 0;
      currentIndex = -1;
      editor?.commands?.clearSearch?.();
      return;
    }

    editor.commands.setSearchTerm(searchTerm);
    const storage = editor.storage.searchHighlight;
    resultCount = storage?.results?.length ?? 0;
    currentIndex = resultCount > 0 ? (storage?.currentIndex ?? 0) + 1 : 0;
  }

  function handleNext() {
    const editor = getEditor();
    if (!editor) return;
    editor.commands.nextSearchResult();
    const storage = editor.storage.searchHighlight;
    currentIndex = (storage?.currentIndex ?? 0) + 1;
  }

  function handlePrevious() {
    const editor = getEditor();
    if (!editor) return;
    editor.commands.previousSearchResult();
    const storage = editor.storage.searchHighlight;
    currentIndex = (storage?.currentIndex ?? 0) + 1;
  }

  function handleClose() {
    open = false;
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      if (e.shiftKey) {
        handlePrevious();
      } else {
        if (resultCount === 0 && searchTerm) {
          handleSearch();
        } else {
          handleNext();
        }
      }
    }
    if (e.key === "Escape") {
      e.preventDefault();
      handleClose();
    }
  }

  // Closing animation state for mobile
  let closing = $state(false);
  let opening = $state(false);
  const showMobileSheet = $derived(open || closing);

  $effect(() => {
    if (open && mobileState.isMobile) {
      opening = true;
      requestAnimationFrame(() => {
        requestAnimationFrame(() => {
          opening = false;
        });
      });
    }
  });

  function closeMobileWithAnimation() {
    if (closing) return;
    closing = true;
    setTimeout(() => {
      open = false;
      closing = false;
    }, 300);
  }
</script>

{#if mobileState.isMobile}
  <!-- Mobile: Bottom drawer -->
  {#if showMobileSheet}
    <!-- Sheet (no backdrop — user needs to see highlights in the document) -->
    <div
      class="fixed inset-x-0 bottom-0 z-50 rounded-t-lg border-t bg-background overflow-hidden"
      style="transform: translateY({closing || opening ? '100%' : '0'});
             transition: transform 0.3s ease-in-out;"
    >
      <div class="px-4 py-3 pb-[calc(env(safe-area-inset-bottom)+0.75rem)]">
        <div class="flex items-center gap-2">
          <Input
            type="text"
            placeholder="Find in document..."
            bind:value={searchTerm}
            bind:ref={inputEl}
            class="flex-1 h-10 text-base"
            oninput={handleSearch}
            onkeydown={handleKeydown}
          />
          <span class="text-xs text-muted-foreground shrink-0 w-12 text-center">
            {#if resultCount > 0}
              {currentIndex}/{resultCount}
            {:else if searchTerm}
              0/0
            {/if}
          </span>
          <Button
            variant="ghost"
            size="icon"
            class="size-11 shrink-0"
            onclick={handlePrevious}
            disabled={resultCount === 0}
            aria-label="Previous match"
          >
            <ChevronUp class="size-5" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            class="size-11 shrink-0"
            onclick={handleNext}
            disabled={resultCount === 0}
            aria-label="Next match"
          >
            <ChevronDown class="size-5" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            class="size-11 shrink-0"
            onclick={closeMobileWithAnimation}
            aria-label="Close find"
          >
            <X class="size-5" />
          </Button>
        </div>
      </div>
    </div>
  {/if}
{:else}
  <!-- Desktop: Floating bar at top of editor -->
  {#if open}
    <div class="absolute top-2 right-4 z-30 flex items-center gap-1.5 bg-popover border rounded-lg shadow-lg px-2 py-1.5">
      <Input
        type="text"
        placeholder="Find..."
        bind:value={searchTerm}
        bind:ref={inputEl}
        class="h-7 w-52 text-sm"
        oninput={handleSearch}
        onkeydown={handleKeydown}
      />
      <span class="text-xs text-muted-foreground shrink-0 w-10 text-center">
        {#if resultCount > 0}
          {currentIndex}/{resultCount}
        {:else if searchTerm}
          0/0
        {/if}
      </span>
      <Button
        variant="ghost"
        size="icon"
        class="size-7 shrink-0"
        onclick={handlePrevious}
        disabled={resultCount === 0}
        aria-label="Previous match"
      >
        <ChevronUp class="size-4" />
      </Button>
      <Button
        variant="ghost"
        size="icon"
        class="size-7 shrink-0"
        onclick={handleNext}
        disabled={resultCount === 0}
        aria-label="Next match"
      >
        <ChevronDown class="size-4" />
      </Button>
      <Button
        variant="ghost"
        size="icon"
        class="size-7 shrink-0"
        onclick={handleClose}
        aria-label="Close find"
      >
        <X class="size-4" />
      </Button>
    </div>
  {/if}
{/if}
