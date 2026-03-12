<script lang="ts">
  import { onMount } from "svelte";
  import { Button } from "$lib/components/ui/button";
  import type { SpotlightStep } from "$lib/marketplace/types";

  interface Props {
    steps: SpotlightStep[];
    onComplete: () => void;
  }

  let { steps, onComplete }: Props = $props();

  let currentStepIndex = $state(0);
  let targetRect = $state<DOMRect | null>(null);
  let overlayEl = $state<HTMLDivElement | null>(null);

  let currentStep = $derived(steps[currentStepIndex] ?? null);
  let isLastStep = $derived(currentStepIndex === steps.length - 1);

  let resizeObserver: ResizeObserver | null = null;
  let currentTarget: HTMLElement | null = null;

  function findTarget(key: string): HTMLElement | null {
    return document.querySelector<HTMLElement>(`[data-spotlight="${key}"]`);
  }

  function updateTargetRect() {
    if (currentTarget) {
      targetRect = currentTarget.getBoundingClientRect();
    }
  }

  function observeTarget(el: HTMLElement | null) {
    resizeObserver?.disconnect();
    currentTarget = el;
    if (el) {
      targetRect = el.getBoundingClientRect();
      resizeObserver = new ResizeObserver(updateTargetRect);
      resizeObserver.observe(el);
    } else {
      targetRect = null;
    }
  }

  function goToStep(index: number) {
    if (index < 0 || index >= steps.length) return;
    currentStepIndex = index;
    const step = steps[index];
    const el = findTarget(step.target);
    if (!el) {
      // Skip steps whose target isn't in the DOM
      if (index < steps.length - 1) {
        goToStep(index + 1);
      } else {
        onComplete();
      }
      return;
    }
    observeTarget(el);
  }

  function next() {
    if (isLastStep) {
      onComplete();
    } else {
      goToStep(currentStepIndex + 1);
    }
  }

  function prev() {
    if (currentStepIndex > 0) {
      goToStep(currentStepIndex - 1);
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onComplete();
    } else if (e.key === "ArrowRight" || e.key === "Enter") {
      e.preventDefault();
      next();
    } else if (e.key === "ArrowLeft") {
      e.preventDefault();
      prev();
    }
  }

  function handleScroll() {
    updateTargetRect();
  }

  onMount(() => {
    // Skip on mobile
    if (window.innerWidth < 768) {
      onComplete();
      return;
    }

    goToStep(0);

    window.addEventListener("resize", updateTargetRect);
    window.addEventListener("scroll", handleScroll, true);

    return () => {
      resizeObserver?.disconnect();
      window.removeEventListener("resize", updateTargetRect);
      window.removeEventListener("scroll", handleScroll, true);
    };
  });

  // Cutout dimensions with padding
  const PAD = 8;
  const RADIUS = 8;

  let cutoutX = $derived(targetRect ? targetRect.left - PAD : 0);
  let cutoutY = $derived(targetRect ? targetRect.top - PAD : 0);
  let cutoutW = $derived(targetRect ? targetRect.width + PAD * 2 : 0);
  let cutoutH = $derived(targetRect ? targetRect.height + PAD * 2 : 0);

  // Tooltip positioning
  const TOOLTIP_GAP = 12;
  const TOOLTIP_WIDTH = 320;
  const TOOLTIP_HEIGHT_ESTIMATE = 140; // approximate height for clamping
  const VIEWPORT_MARGIN = 16;

  let tooltipStyle = $derived.by(() => {
    if (!targetRect || !currentStep) return "display: none";

    const { placement } = currentStep;
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    let top = 0;
    let left = 0;

    switch (placement) {
      case "right":
        top = targetRect.top + targetRect.height / 2 - TOOLTIP_HEIGHT_ESTIMATE / 2;
        left = targetRect.right + PAD + TOOLTIP_GAP;
        break;
      case "left":
        top = targetRect.top + targetRect.height / 2 - TOOLTIP_HEIGHT_ESTIMATE / 2;
        left = targetRect.left - PAD - TOOLTIP_GAP - TOOLTIP_WIDTH;
        break;
      case "bottom":
        top = targetRect.bottom + PAD + TOOLTIP_GAP;
        left = targetRect.left + targetRect.width / 2 - TOOLTIP_WIDTH / 2;
        break;
      case "top":
        top = targetRect.top - PAD - TOOLTIP_GAP - TOOLTIP_HEIGHT_ESTIMATE;
        left = targetRect.left + targetRect.width / 2 - TOOLTIP_WIDTH / 2;
        break;
      default:
        return "display: none";
    }

    // Clamp to viewport
    top = Math.max(VIEWPORT_MARGIN, Math.min(top, vh - TOOLTIP_HEIGHT_ESTIMATE - VIEWPORT_MARGIN));
    left = Math.max(VIEWPORT_MARGIN, Math.min(left, vw - TOOLTIP_WIDTH - VIEWPORT_MARGIN));

    return `top: ${top}px; left: ${left}px; width: ${TOOLTIP_WIDTH}px`;
  });
</script>

<svelte:window onkeydown={handleKeydown} />

{#if targetRect && currentStep}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    bind:this={overlayEl}
    class="spotlight-overlay"
    onclick={(e) => { if (e.target === overlayEl) next(); }}
    onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') next(); }}
    role="presentation"
  >
    <!-- Backdrop with cutout -->
    <svg class="spotlight-backdrop" aria-hidden="true">
      <defs>
        <mask id="spotlight-mask">
          <rect x="0" y="0" width="100%" height="100%" fill="white" />
          <rect
            x={cutoutX}
            y={cutoutY}
            width={cutoutW}
            height={cutoutH}
            rx={RADIUS}
            ry={RADIUS}
            fill="black"
          />
        </mask>
      </defs>
      <rect
        x="0" y="0" width="100%" height="100%"
        fill="rgba(0, 0, 0, 0.5)"
        mask="url(#spotlight-mask)"
      />
    </svg>

    <!-- Tooltip -->
    <div
      class="spotlight-tooltip"
      style={tooltipStyle}
      role="dialog"
      aria-label="Onboarding tour"
    >
      <div class="space-y-2">
        <h3 class="text-sm font-semibold text-foreground">{currentStep.title}</h3>
        <p class="text-xs text-muted-foreground leading-relaxed">{currentStep.description}</p>
      </div>

      <div class="flex items-center justify-between mt-4">
        <span class="text-xs text-muted-foreground/60">
          {currentStepIndex + 1} of {steps.length}
        </span>
        <div class="flex items-center gap-2">
          <Button variant="ghost" size="sm" onclick={onComplete}>
            Skip
          </Button>
          <Button size="sm" onclick={next}>
            {isLastStep ? "Done" : "Next"}
          </Button>
        </div>
      </div>
    </div>
  </div>
{/if}

<style>
  .spotlight-overlay {
    position: fixed;
    inset: 0;
    z-index: 9999;
  }

  .spotlight-backdrop {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    transition: all 0.3s ease-in-out;
  }

  .spotlight-tooltip {
    position: fixed;
    background: var(--popover);
    color: var(--popover-foreground);
    border: 1px solid var(--border);
    border-radius: 0.5rem;
    padding: 1rem;
    box-shadow: 0 10px 25px rgba(0, 0, 0, 0.15);
    transition: top 0.3s ease-in-out, left 0.3s ease-in-out, transform 0.3s ease-in-out;
    animation: tooltipFadeIn 0.2s ease-out;
  }

  @keyframes tooltipFadeIn {
    from {
      opacity: 0;
      scale: 0.95;
    }
    to {
      opacity: 1;
      scale: 1;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .spotlight-backdrop,
    .spotlight-tooltip {
      transition: none;
      animation: none;
    }
  }
</style>
