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
    filter: blur(55px);
    will-change: transform;
    opacity: 0.28;
  }

  /* Wave 1: upper-left, drifts right */
  .wave-1 {
    height: 55%;
    background: linear-gradient(
      to right,
      oklch(0.58 0.22 265) 0%,
      oklch(0.62 0.18 285) 45%,
      oklch(0.65 0.15 305) 100%
    );
    top: -15%;
    left: -55%;
    animation: wave-drift-1 9s ease-in-out infinite alternate;
  }

  /* Wave 2: lower-right, drifts left */
  .wave-2 {
    height: 60%;
    background: linear-gradient(
      to right,
      oklch(0.60 0.20 248) 0%,
      oklch(0.55 0.25 268) 50%,
      oklch(0.58 0.22 290) 100%
    );
    bottom: -22%;
    right: -55%;
    animation: wave-drift-2 11s ease-in-out infinite alternate;
  }

  /* Wave 3: middle, drifts diagonally */
  .wave-3 {
    height: 45%;
    background: linear-gradient(
      to right,
      oklch(0.62 0.18 290) 0%,
      oklch(0.60 0.20 258) 50%,
      oklch(0.56 0.24 275) 100%
    );
    top: 28%;
    left: -25%;
    animation: wave-drift-3 13s ease-in-out infinite alternate;
  }

  @keyframes wave-drift-1 {
    from { transform: translateX(0%) translateY(0%); }
    to   { transform: translateX(40%) translateY(14%); }
  }

  @keyframes wave-drift-2 {
    from { transform: translateX(0%) translateY(0%); }
    to   { transform: translateX(-35%) translateY(-20%); }
  }

  @keyframes wave-drift-3 {
    from { transform: translateX(0%) translateY(0%); }
    to   { transform: translateX(28%) translateY(26%); }
  }

  /* Dark mode: deeper, more vibrant colors at higher opacity */
  :global(.dark) .aurora-wave {
    opacity: 0.42;
  }

  :global(.dark) .wave-1 {
    background: linear-gradient(
      to right,
      oklch(0.45 0.27 265) 0%,
      oklch(0.40 0.30 285) 45%,
      oklch(0.38 0.25 305) 100%
    );
  }

  :global(.dark) .wave-2 {
    background: linear-gradient(
      to right,
      oklch(0.35 0.22 245) 0%,
      oklch(0.42 0.27 265) 50%,
      oklch(0.38 0.30 285) 100%
    );
  }

  :global(.dark) .wave-3 {
    background: linear-gradient(
      to right,
      oklch(0.40 0.25 285) 0%,
      oklch(0.48 0.23 260) 50%,
      oklch(0.42 0.27 270) 100%
    );
  }

  /* Respect reduced-motion preference */
  @media (prefers-reduced-motion: reduce) {
    .aurora-wave {
      animation: none;
    }
  }
</style>
