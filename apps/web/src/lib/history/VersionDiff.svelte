<script lang="ts">
  type ChangeType = 'Added' | 'Modified' | 'Deleted' | 'Restored' | string;

  interface FileDiff {
    path: string;
    change_type: ChangeType;
    old_value?: string | null;
    new_value?: string | null;
  }

  interface Props {
    diffs: FileDiff[];
  }

  let { diffs }: Props = $props();

  function getChangeIcon(changeType: ChangeType): string {
    switch (changeType) {
      case 'Added':
        return '+';
      case 'Modified':
        return '~';
      case 'Deleted':
        return '-';
      case 'Restored':
        return 'R';
      default:
        return '?';
    }
  }

  function getChangeClass(changeType: ChangeType): string {
    switch (changeType) {
      case 'Added':
        return 'bg-emerald-600 text-white';
      case 'Modified':
        return 'bg-amber-500 text-black';
      case 'Deleted':
        return 'bg-red-600 text-white';
      case 'Restored':
        return 'bg-sky-500 text-white';
      default:
        return 'bg-muted text-muted-foreground';
    }
  }

  function getChangeLabel(changeType: ChangeType): string {
    switch (changeType) {
      case 'Added':
        return 'Added';
      case 'Modified':
        return 'Modified';
      case 'Deleted':
        return 'Deleted';
      case 'Restored':
        return 'Restored';
      default:
        return String(changeType);
    }
  }

  function getFileName(path: string): string {
    return path.split('/').pop() || path;
  }
</script>

<div class="text-sm">
  {#if diffs.length === 0}
    <div class="px-4 py-4 text-center text-muted-foreground">No changes in this version</div>
  {:else}
    <div class="flex flex-col gap-1">
      {#each diffs as diff}
        <div class="flex items-center gap-2 rounded-md bg-muted px-2 py-1.5">
          <span
            class={`flex h-[1.2rem] w-[1.2rem] shrink-0 items-center justify-center rounded-[4px] text-[0.8rem] font-semibold ${getChangeClass(diff.change_type)}`}
          >
            {getChangeIcon(diff.change_type)}
          </span>
          <span class="min-w-0 flex-1 truncate text-foreground" title={diff.path}>
            {getFileName(diff.path)}
          </span>
          <span class="shrink-0 text-xs text-muted-foreground">{getChangeLabel(diff.change_type)}</span>
        </div>
      {/each}
    </div>
  {/if}
</div>
