<script lang="ts">
  /**
   * WelcomeScreen — full-screen first-run experience shown when no workspaces exist.
   *
   * Shows a welcome message with a "Get Started" button that opens the
   * AddWorkspaceDialog, where the user chooses workspace name, location,
   * sync mode, and content source.
   */
  import { onMount } from "svelte";
  import { Button } from "$lib/components/ui/button";

  interface Props {
    onGetStarted: () => void;
  }

  let { onGetStarted }: Props = $props();

  let canvas: HTMLCanvasElement;

  onMount(() => {
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;

    const ctx = canvas.getContext("2d")!;
    let animFrameId: number;
    let startTime: number | null = null;

    const SPACING = 30;   // px between dots (logical)
    const BASE_R = 1.0;   // dot radius at trough
    const PEAK_R = 2.6;   // dot radius at crest
    const FREQ = 0.052;   // spatial frequency — tighter = more rings visible
    const SPEED = 0.9;    // radians per second — wave travel speed

    function resize() {
      canvas.width = canvas.offsetWidth * devicePixelRatio;
      canvas.height = canvas.offsetHeight * devicePixelRatio;
    }

    const ro = new ResizeObserver(resize);
    ro.observe(canvas);
    resize();

    function draw(ts: number) {
      if (startTime === null) startTime = ts;
      const t = (ts - startTime) / 1000;

      const { width, height } = canvas;
      ctx.clearRect(0, 0, width, height);

      const dpr = devicePixelRatio;
      const cx = width / 2;
      const cy = height / 2;
      const isDark = document.documentElement.classList.contains("dark");

      // Blue-indigo tones that complement the app palette in both modes
      const [r, g, b] = isDark ? [160, 172, 215] : [90, 100, 175];
      const spacing = SPACING * dpr;

      // Align grid so it's centered on the canvas
      const cols = Math.ceil(width / spacing) + 2;
      const rows = Math.ceil(height / spacing) + 2;
      const startX = cx - Math.floor(cols / 2) * spacing;
      const startY = cy - Math.floor(rows / 2) * spacing;

      for (let row = 0; row < rows; row++) {
        for (let col = 0; col < cols; col++) {
          const x = startX + col * spacing;
          const y = startY + row * spacing;
          const dist = Math.sqrt((x - cx) ** 2 + (y - cy) ** 2);

          // Outward-propagating sine wave; crest = 1, trough = 0
          const wave = (Math.sin(dist * FREQ - t * SPEED) + 1) / 2;

          const dotR = (BASE_R + wave * (PEAK_R - BASE_R)) * dpr;
          const alpha = isDark
            ? 0.10 + wave * 0.38
            : 0.08 + wave * 0.30;

          ctx.beginPath();
          ctx.arc(x, y, dotR, 0, Math.PI * 2);
          ctx.fillStyle = `rgba(${r},${g},${b},${alpha.toFixed(3)})`;
          ctx.fill();
        }
      }

      animFrameId = requestAnimationFrame(draw);
    }

    animFrameId = requestAnimationFrame(draw);

    return () => {
      cancelAnimationFrame(animFrameId);
      ro.disconnect();
    };
  });
</script>

<div class="relative flex items-center justify-center min-h-full bg-background px-4 overflow-hidden">
  <canvas bind:this={canvas} class="absolute inset-0 w-full h-full" aria-hidden="true"></canvas>

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
