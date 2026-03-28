<script lang="ts">
  interface Props {
    email: string;
    onVerified?: () => void;
    onError?: (message: string) => void;
  }

  let { email, onVerified, onError }: Props = $props();
</script>

<div data-email={email}>
  <button type="button" onclick={() => onVerified?.()}>Verify</button>
  <button type="button" onclick={() => onError?.("Verification failed")}>Fail Verify</button>
</div>
