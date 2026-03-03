<script lang="ts">
  /**
   * PluginSettingsTab - Renders declarative settings fields from a plugin manifest.
   *
   * Takes an array of SettingsField definitions and renders them as form controls
   * using shadcn-svelte primitives. Calls onConfigChange when any field value changes.
   */
  import { Switch } from "$lib/components/ui/switch";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import type { SettingsField, SelectOption } from "$lib/backend/generated";
  import type { JsonValue } from "$lib/backend/generated/serde_json/JsonValue";

  interface Props {
    fields: SettingsField[];
    config: Record<string, JsonValue>;
    onConfigChange: (key: string, value: JsonValue) => void;
  }

  let { fields, config, onConfigChange }: Props = $props();
</script>

<div class="space-y-4">
  {#each fields as field}
    {#if field.type === "Section"}
      <div class="pt-2">
        <h4 class="font-medium text-sm">{field.label}</h4>
        {#if field.description}
          <p class="text-xs text-muted-foreground">{field.description}</p>
        {/if}
      </div>
    {:else if field.type === "Toggle"}
      <div class="flex items-center justify-between gap-4 px-1">
        <Label for={field.key} class="text-sm cursor-pointer flex flex-col gap-0.5">
          <span>{field.label}</span>
          {#if field.description}
            <span class="font-normal text-xs text-muted-foreground">{field.description}</span>
          {/if}
        </Label>
        <Switch
          id={field.key}
          checked={config[field.key] === true}
          onCheckedChange={(checked) => onConfigChange(field.key, checked)}
        />
      </div>
    {:else if field.type === "Text"}
      <div class="space-y-1.5 px-1">
        <Label for={field.key} class="text-sm flex flex-col gap-0.5">
          <span>{field.label}</span>
          {#if field.description}
            <span class="font-normal text-xs text-muted-foreground">{field.description}</span>
          {/if}
        </Label>
        <Input
          id={field.key}
          type="text"
          value={(config[field.key] as string) ?? ""}
          oninput={(e) => onConfigChange(field.key, (e.target as HTMLInputElement).value)}
        />
      </div>
    {:else if field.type === "Number"}
      <div class="space-y-1.5 px-1">
        <Label for={field.key} class="text-sm">{field.label}</Label>
        <Input
          id={field.key}
          type="number"
          min={field.min ?? undefined}
          max={field.max ?? undefined}
          value={(config[field.key] as number) ?? 0}
          oninput={(e) => onConfigChange(field.key, Number((e.target as HTMLInputElement).value))}
        />
      </div>
    {:else if field.type === "Select"}
      <div class="space-y-1.5 px-1">
        <Label for={field.key} class="text-sm flex flex-col gap-0.5">
          <span>{field.label}</span>
          {#if field.description}
            <span class="font-normal text-xs text-muted-foreground">{field.description}</span>
          {/if}
        </Label>
        <select
          id={field.key}
          class="w-full px-2 py-1 text-sm border rounded bg-background"
          value={(config[field.key] as string) ?? ""}
          onchange={(e) => onConfigChange(field.key, (e.target as HTMLSelectElement).value)}
        >
          {#each (field.options as SelectOption[]) as opt}
            <option value={opt.value}>{opt.label}</option>
          {/each}
        </select>
      </div>
    {/if}
  {/each}
</div>
