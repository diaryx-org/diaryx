<script lang="ts">
  /**
   * WelcomeScreen — full-screen first-run experience shown when no workspaces exist.
   *
   * Shows a welcome message with a "Get Started" button that opens the
   * AddWorkspaceDialog, where the user chooses workspace name, location,
   * sync mode, and content source.
   */
  import { Button } from "$lib/components/ui/button";

  interface Props {
    onGetStarted: () => void;
  }

  let { onGetStarted }: Props = $props();
</script>

<div class="flex items-center justify-center min-h-full bg-background px-4">
  <div class="w-full max-w-sm space-y-6">
    <div class="text-center space-y-4">
      <img src="/icon.png" alt="Diaryx" class="size-16 mx-auto fade-in" style="animation-delay: 0s" />
      <h1 class="text-3xl font-bold tracking-tight text-foreground fade-in" style="animation-delay: 0.2s">
        Welcome to Diaryx
      </h1>
      <p class="text-muted-foreground text-sm fade-in" style="animation-delay: 0.4s">
        Diaryx keeps your notes portable and powerful. <br/>Create a workspace to begin.
      </p>
    </div>

    <Button
      class="w-full fade-in"
      style="animation-delay: 0.6s"
      onclick={onGetStarted}
    >
      Get Started
    </Button>

  </div>
</div>

<style>
  @keyframes fadeIn {
    from {
      opacity: 0;
      transform: translateY(10px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  .fade-in {
    animation: fadeIn 0.4s ease-out both;
  }
</style>
