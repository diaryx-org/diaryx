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

<div class="aurora-screen flex items-center justify-center min-h-full px-4">
  <!-- Background aurora animation -->
  <div class="aurora-bg" aria-hidden="true">
    <div class="aurora-wave wave-1"></div>
    <div class="aurora-wave wave-2"></div>
    <div class="aurora-wave wave-3"></div>
  </div>

  <div class="relative z-10 w-full max-w-sm space-y-6">
    <div class="text-center space-y-4">
      <img src="/icon.png" alt="Diaryx" class="size-16 mx-auto" />
      <h1 class="text-3xl font-bold tracking-tight text-foreground">
        Welcome to Diaryx
      </h1>
      <p class="text-muted-foreground text-sm">
        Diaryx keeps your notes portable and powerful. <br/>Create a workspace to begin.
      </p>
    </div>

    <Button
      class="w-full"
      onclick={onGetStarted}
    >
      Get Started
    </Button>

  </div>
</div>

<style>
  .aurora-screen {
    position: relative;
    overflow: hidden;
    background: var(--background);
  }

  .aurora-bg {
    position: absolute;
    inset: 0;
    pointer-events: none;
  }

  .aurora-wave {
    position: absolute;
    width: 200%;
    border-radius: 50%;
    filter: blur(72px);
    will-change: transform;
    opacity: 0.18;
  }

  /* Wave 1: upper-left, drifts right */
  .wave-1 {
    height: 55%;
    background: linear-gradient(
      to right,
      oklch(0.72 0.14 265) 0%,
      oklch(0.78 0.10 285) 45%,
      oklch(0.80 0.08 305) 100%
    );
    top: -15%;
    left: -55%;
    animation: wave-drift-1 14s ease-in-out infinite alternate;
  }

  /* Wave 2: lower-right, drifts left */
  .wave-2 {
    height: 60%;
    background: linear-gradient(
      to right,
      oklch(0.82 0.07 248) 0%,
      oklch(0.75 0.12 268) 50%,
      oklch(0.70 0.15 290) 100%
    );
    bottom: -22%;
    right: -55%;
    animation: wave-drift-2 17s ease-in-out infinite alternate;
  }

  /* Wave 3: middle, drifts diagonally */
  .wave-3 {
    height: 45%;
    background: linear-gradient(
      to right,
      oklch(0.76 0.10 290) 0%,
      oklch(0.80 0.08 258) 50%,
      oklch(0.74 0.13 275) 100%
    );
    top: 28%;
    left: -25%;
    animation: wave-drift-3 21s ease-in-out infinite alternate;
  }

  @keyframes wave-drift-1 {
    from { transform: translateX(0%) translateY(0%); }
    to   { transform: translateX(25%) translateY(8%); }
  }

  @keyframes wave-drift-2 {
    from { transform: translateX(0%) translateY(0%); }
    to   { transform: translateX(-20%) translateY(-12%); }
  }

  @keyframes wave-drift-3 {
    from { transform: translateX(0%) translateY(0%); }
    to   { transform: translateX(16%) translateY(16%); }
  }

  /* Dark mode: deeper, more vibrant colors at higher opacity */
  :global(.dark) .aurora-wave {
    opacity: 0.30;
  }

  :global(.dark) .wave-1 {
    background: linear-gradient(
      to right,
      oklch(0.45 0.22 265) 0%,
      oklch(0.40 0.25 285) 45%,
      oklch(0.38 0.20 305) 100%
    );
  }

  :global(.dark) .wave-2 {
    background: linear-gradient(
      to right,
      oklch(0.35 0.18 245) 0%,
      oklch(0.42 0.22 265) 50%,
      oklch(0.38 0.25 285) 100%
    );
  }

  :global(.dark) .wave-3 {
    background: linear-gradient(
      to right,
      oklch(0.40 0.20 285) 0%,
      oklch(0.48 0.18 260) 50%,
      oklch(0.42 0.22 270) 100%
    );
  }

  /* Respect reduced-motion preference */
  @media (prefers-reduced-motion: reduce) {
    .aurora-wave {
      animation: none;
    }
  }
</style>
