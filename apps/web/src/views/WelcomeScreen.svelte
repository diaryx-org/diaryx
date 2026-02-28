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

<div class="flex items-center justify-center min-h-full welcome-bg px-4">
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
      class="w-full fade-in get-started-btn"
      style="animation-delay: 0.6s"
      onclick={onGetStarted}
    >
      Get Started
    </Button>

  </div>
</div>

<style>
  @property --orb1-x { syntax: '<percentage>'; initial-value: 20%; inherits: false; }
  @property --orb1-y { syntax: '<percentage>'; initial-value: 25%; inherits: false; }
  @property --orb2-x { syntax: '<percentage>'; initial-value: 75%; inherits: false; }
  @property --orb2-y { syntax: '<percentage>'; initial-value: 65%; inherits: false; }

  @keyframes ambientShift {
    0%   { --orb1-x: 20%; --orb1-y: 25%; --orb2-x: 75%; --orb2-y: 65%; }
    25%  { --orb1-x: 55%; --orb1-y: 40%; --orb2-x: 30%; --orb2-y: 35%; }
    50%  { --orb1-x: 45%; --orb1-y: 65%; --orb2-x: 60%; --orb2-y: 25%; }
    75%  { --orb1-x: 25%; --orb1-y: 55%; --orb2-x: 70%; --orb2-y: 60%; }
    100% { --orb1-x: 20%; --orb1-y: 25%; --orb2-x: 75%; --orb2-y: 65%; }
  }

  .welcome-bg {
    background-color: var(--background);
    background-image:
      radial-gradient(
        ellipse 70% 60% at var(--orb1-x) var(--orb1-y),
        color-mix(in oklch, var(--primary) 12%, transparent) 0%,
        transparent 60%
      ),
      radial-gradient(
        ellipse 60% 70% at var(--orb2-x) var(--orb2-y),
        color-mix(in oklch, var(--ring) 8%, transparent) 0%,
        transparent 60%
      );
    animation: ambientShift 22s ease-in-out infinite;
  }

  @media (prefers-reduced-motion: reduce) {
    .welcome-bg { animation: none; }
  }

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
    animation: fadeIn 0.4s ease-out backwards;
  }

  :global(.get-started-btn) {
    transition: transform 0.2s ease-out, box-shadow 0.2s ease-out;
  }

  :global(.get-started-btn:hover) {
    transform: scale(1.02);
    box-shadow: 0 4px 20px color-mix(in oklch, var(--primary) 35%, transparent);
  }
</style>
