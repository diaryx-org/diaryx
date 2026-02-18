<script lang="ts">
  import { verifyCode } from "$lib/auth/authStore.svelte";
  import { Loader2 } from "@lucide/svelte";

  interface Props {
    email: string;
    onVerified: () => void;
    onError: (msg: string) => void;
  }

  let { email, onVerified, onError }: Props = $props();

  let code = $state("");
  let isVerifying = $state(false);

  async function handleSubmit() {
    if (code.length !== 6 || isVerifying) return;
    isVerifying = true;
    try {
      await verifyCode(code, email);
      onVerified();
    } catch (err) {
      const msg = err instanceof Error ? err.message : "Invalid code";
      onError(msg);
      code = "";
    } finally {
      isVerifying = false;
    }
  }

  function handleInput(e: Event) {
    const input = e.target as HTMLInputElement;
    // Strip non-digits
    code = input.value.replace(/\D/g, "").slice(0, 6);
    input.value = code;
    if (code.length === 6) {
      handleSubmit();
    }
  }

  function handlePaste(e: ClipboardEvent) {
    const text = e.clipboardData?.getData("text") || "";
    const digits = text.replace(/\D/g, "").slice(0, 6);
    if (digits.length > 0) {
      e.preventDefault();
      code = digits;
      (e.target as HTMLInputElement).value = code;
      if (code.length === 6) {
        handleSubmit();
      }
    }
  }
</script>

<div class="space-y-1.5">
  <p class="text-xs text-muted-foreground text-center">Or enter the code from your email:</p>
  <div class="flex justify-center">
    <div class="relative">
      <input
        type="text"
        inputmode="numeric"
        maxlength="6"
        pattern="[0-9]*"
        autocomplete="one-time-code"
        placeholder="000000"
        value={code}
        oninput={handleInput}
        onpaste={handlePaste}
        disabled={isVerifying}
        class="w-[10rem] text-center tracking-[0.3em] font-mono text-lg border rounded-md px-3 py-2 bg-background focus:outline-none focus:ring-2 focus:ring-ring disabled:opacity-50"
      />
      {#if isVerifying}
        <div class="absolute right-2 top-1/2 -translate-y-1/2">
          <Loader2 class="size-4 animate-spin text-muted-foreground" />
        </div>
      {/if}
    </div>
  </div>
</div>
