<script lang="ts">
  interface Props {
    id?: string;
    checked?: boolean;
    disabled?: boolean;
    onCheckedChange?: (checked: boolean) => void;
  }

  let { id, checked = false, disabled = false, onCheckedChange }: Props = $props();
</script>

<input
  {id}
  type="checkbox"
  checked={checked}
  {disabled}
  onchange={(event) => onCheckedChange?.((event.currentTarget as HTMLInputElement).checked)}
/>
