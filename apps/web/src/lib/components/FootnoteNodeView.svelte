<script lang="ts">
  interface Props {
    label: string;
    content: string;
    readonly: boolean;
    onUpdate: (content: string) => void;
  }

  let { label, content, readonly, onUpdate }: Props = $props();

  let showPopover = $state(false);
  let editContent = $state("");
  let inputElement: HTMLInputElement | undefined = $state();

  function handleClick() {
    if (readonly) return;
    editContent = content;
    showPopover = !showPopover;
    if (!showPopover) return;
    // Focus input after mount
    requestAnimationFrame(() => {
      inputElement?.focus();
    });
  }

  function handleSave() {
    onUpdate(editContent);
    showPopover = false;
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      handleSave();
    } else if (e.key === "Escape") {
      e.preventDefault();
      editContent = content;
      showPopover = false;
    }
  }

  function handleClickOutside(e: MouseEvent) {
    const target = e.target as HTMLElement;
    if (!target.closest(".footnote-popover") && !target.closest(".footnote-label")) {
      handleSave();
    }
  }

  $effect(() => {
    if (showPopover) {
      const timeoutId = setTimeout(() => {
        document.addEventListener("mousedown", handleClickOutside);
      }, 0);
      return () => {
        clearTimeout(timeoutId);
        document.removeEventListener("mousedown", handleClickOutside);
      };
    }
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<span class="footnote-node">
  <span
    class="footnote-label"
    title={content || "Empty footnote"}
    onclick={handleClick}
  >{label}</span>
  {#if showPopover}
    <span class="footnote-popover">
      <input
        bind:this={inputElement}
        type="text"
        class="footnote-input"
        bind:value={editContent}
        onkeydown={handleKeydown}
        placeholder="Footnote content..."
      />
    </span>
  {/if}
</span>

<style>
  .footnote-node {
    position: relative;
    display: inline;
  }

  .footnote-label {
    font-size: 0.75em;
    vertical-align: super;
    color: var(--primary);
    cursor: pointer;
    font-weight: 600;
    line-height: 1;
  }

  .footnote-label:hover {
    opacity: 0.8;
  }

  .footnote-popover {
    position: absolute;
    top: 100%;
    left: 50%;
    transform: translateX(-50%);
    z-index: 100;
    margin-top: 4px;
    background: var(--popover);
    border: 1px solid var(--border);
    border-radius: 6px;
    box-shadow:
      0 10px 15px -3px rgba(0, 0, 0, 0.1),
      0 4px 6px -2px rgba(0, 0, 0, 0.05);
    padding: 6px;
    min-width: 200px;
  }

  .footnote-input {
    width: 100%;
    padding: 4px 8px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--background);
    color: var(--foreground);
    font-size: 13px;
    outline: none;
  }

  .footnote-input:focus {
    border-color: var(--primary);
  }
</style>
