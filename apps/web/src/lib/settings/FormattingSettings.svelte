<script lang="ts">
  /**
   * FormattingSettings - Editor formatting preferences section
   *
   * Controls custom formatting options like spoiler syntax.
   */
  import { Switch } from "$lib/components/ui/switch";
  import { Label } from "$lib/components/ui/label";
  import { Type } from "@lucide/svelte";
  import { getFormattingStore } from "../stores/formattingStore.svelte";

  const formattingStore = getFormattingStore();
</script>

<div class="space-y-3">
  <h3 class="font-medium flex items-center gap-2">
    <Type class="size-4" />
    Formatting
  </h3>

  <!-- Spoiler Syntax -->
  <div class="flex items-center justify-between gap-4 px-1">
    <Label for="enable-spoilers" class="text-sm cursor-pointer flex flex-col gap-0.5">
      <span>Spoiler syntax</span>
      <span class="font-normal text-xs text-muted-foreground">
        Enable Discord-style ||spoiler|| syntax to hide text until clicked.
      </span>
    </Label>
    <Switch
      id="enable-spoilers"
      checked={formattingStore.enableSpoilers}
      onCheckedChange={(checked) => formattingStore.setEnableSpoilers(checked)}
    />
  </div>
</div>
