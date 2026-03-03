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
  import { Button } from "$lib/components/ui/button";
  import { Loader2, Check, AlertCircle } from "@lucide/svelte";
  import { dispatchCommand, getPlugin } from "$lib/plugins/browserPluginManager.svelte";
  import type { SettingsField, SelectOption } from "$lib/backend/generated";
  import type { JsonValue } from "$lib/backend/generated/serde_json/JsonValue";

  interface Props {
    pluginId: string;
    fields: SettingsField[];
    config: Record<string, JsonValue>;
    onConfigChange: (key: string, value: JsonValue) => void;
  }

  let { pluginId, fields, config, onConfigChange }: Props = $props();

  // Button state: keyed by command name
  let buttonStates = $state<Record<string, { loading: boolean; result?: { success: boolean; message: string } }>>({});

  async function handleButtonClick(command: string) {
    buttonStates = { ...buttonStates, [command]: { loading: true } };
    try {
      // Save current config first
      const browserPlugin = getPlugin(pluginId);
      if (browserPlugin) {
        await browserPlugin.setConfig(config as Record<string, unknown>);
      }
      const result = await dispatchCommand(pluginId, command, {});
      if (result.success) {
        buttonStates = { ...buttonStates, [command]: { loading: false, result: { success: true, message: "Success" } } };
      } else {
        buttonStates = { ...buttonStates, [command]: { loading: false, result: { success: false, message: result.error ?? "Failed" } } };
      }
    } catch (e) {
      buttonStates = { ...buttonStates, [command]: { loading: false, result: { success: false, message: String(e) } } };
    }
  }
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
          placeholder={field.placeholder ?? undefined}
          value={(config[field.key] as string) ?? ""}
          oninput={(e) => onConfigChange(field.key, (e.target as HTMLInputElement).value)}
        />
      </div>
    {:else if field.type === "Password"}
      <div class="space-y-1.5 px-1">
        <Label for={field.key} class="text-sm flex flex-col gap-0.5">
          <span>{field.label}</span>
          {#if field.description}
            <span class="font-normal text-xs text-muted-foreground">{field.description}</span>
          {/if}
        </Label>
        <Input
          id={field.key}
          type="password"
          placeholder={field.placeholder ?? undefined}
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
    {:else if field.type === "Button"}
      {@const state = buttonStates[field.command]}
      <div class="flex items-center gap-3 px-1 pt-1">
        <Button
          variant={field.variant === "destructive" ? "destructive" : field.variant === "outline" ? "outline" : "default"}
          disabled={state?.loading}
          onclick={() => handleButtonClick(field.command)}
        >
          {#if state?.loading}
            <Loader2 class="size-4 mr-1.5 animate-spin" />
          {/if}
          {field.label}
        </Button>
        {#if state?.result}
          <span class="flex items-center gap-1 text-sm" class:text-green-600={state.result.success} class:text-destructive={!state.result.success}>
            {#if state.result.success}
              <Check class="size-4" />
            {:else}
              <AlertCircle class="size-4" />
            {/if}
            {state.result.message}
          </span>
        {/if}
      </div>
    {/if}
  {/each}
</div>
