<script lang="ts">
  import { onMount } from "svelte";
  import { Button } from "$lib/components/ui/button";
  import { getMobileState } from "$lib/hooks/useMobile.svelte";
  import type { SpotlightStep } from "$lib/marketplace/types";

  interface MobileTargetAction {
    prepare: () => Promise<(() => void) | null>;
  }

  interface Props {
    steps: SpotlightStep[];
    onComplete: () => void;
    mobileTargetActions?: Record<string, MobileTargetAction>;
  }

  let { steps, onComplete, mobileTargetActions = {} }: Props = $props();

  let currentStepIndex = $state(0);
  let targetRect = $state<DOMRect | null>(null);
  let overlayEl = $state<HTMLDivElement | null>(null);
  let isMobileMode = $state(false);
  let transitioning = $state(false);

  let currentStep = $derived(steps[currentStepIndex] ?? null);
  let isLastStep = $derived(currentStepIndex === steps.length - 1);

  let resizeObserver: ResizeObserver | null = null;
  let currentTarget: HTMLElement | null = null;
  let currentCleanup: (() => void) | null = null;

  // Touch swipe state
  let touchStartX = 0;
  let touchStartY = 0;

  function findTarget(key: string): HTMLElement | null {
    return document.querySelector<HTMLElement>(`[data-spotlight="${key}"]`);
  }

  function updateTargetRect() {
    if (currentTarget) {
      const rect = currentTarget.getBoundingClientRect();
      // On mobile, detect if sidebar was closed externally (element has no dimensions)
      if (isMobileMode && rect.width === 0 && rect.height === 0) {
        next();
        return;
      }
      targetRect = rect;
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

  async function goToStep(index: number) {
    if (index < 0 || index >= steps.length) return;
    currentStepIndex = index;
    const step = steps[index];

    // On mobile, always clean up the previous step (e.g. close sidebar) before preparing the next
    if (isMobileMode) {
      transitioning = true;
      currentCleanup?.();
      currentCleanup = null;

      if (mobileTargetActions[step.target]) {
        const cleanup = await mobileTargetActions[step.target].prepare();
        currentCleanup = cleanup;
      }

      // Wait for DOM to settle after sidebar animation
      await new Promise<void>(r => requestAnimationFrame(() => r()));
      transitioning = false;
    }

    const el = findTarget(step.target);
    if (!el) {
      // Skip steps whose target isn't in the DOM
      if (index < steps.length - 1) {
        goToStep(index + 1);
      } else {
        await finish();
      }
      return;
    }
    observeTarget(el);
  }

  async function finish() {
    currentCleanup?.();
    currentCleanup = null;
    onComplete();
  }

  function next() {
    if (transitioning) return;
    if (isLastStep) {
      finish();
    } else {
      goToStep(currentStepIndex + 1);
    }
  }

  function prev() {
    if (transitioning) return;
    if (currentStepIndex > 0) {
      goToStep(currentStepIndex - 1);
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      finish();
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

  // Touch swipe handlers for mobile bottom card
  function handleTouchStart(e: TouchEvent) {
    touchStartX = e.touches[0].clientX;
    touchStartY = e.touches[0].clientY;
  }

  function handleTouchEnd(e: TouchEvent) {
    const dx = e.changedTouches[0].clientX - touchStartX;
    const dy = e.changedTouches[0].clientY - touchStartY;
    // Only count horizontal swipes (ignore vertical)
    if (Math.abs(dx) > 50 && Math.abs(dx) > Math.abs(dy)) {
      if (dx < 0) {
        next(); // swipe left = next
      } else {
        prev(); // swipe right = previous
      }
    }
  }

  onMount(() => {
    isMobileMode = getMobileState().isMobile;

    goToStep(0);

    window.addEventListener("resize", updateTargetRect);
    window.addEventListener("scroll", handleScroll, true);

    return () => {
      currentCleanup?.();
      currentCleanup = null;
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

  // On mobile, move the card to the top when the target is in the bottom half of the viewport
  let mobileCardAtTop = $derived(
    isMobileMode && targetRect != null && targetRect.bottom > window.innerHeight / 2
  );

  // Tooltip positioning (desktop only)
  const TOOLTIP_GAP = 12;
  const TOOLTIP_WIDTH = 320;
  const TOOLTIP_HEIGHT_ESTIMATE = 140; // approximate height for clamping
  const VIEWPORT_MARGIN = 16;

  let tooltipStyle = $derived.by(() => {
    if (!targetRect || !currentStep || isMobileMode) return "display: none";

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

    {#if !isMobileMode}
      <!-- Desktop positioned tooltip -->
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
            <Button variant="ghost" size="sm" onclick={() => finish()}>
              Skip
            </Button>
            <Button size="sm" onclick={next}>
              {isLastStep ? "Done" : "Next"}
            </Button>
          </div>
        </div>
      </div>
    {/if}
  </div>

  {#if isMobileMode}
    <!-- Mobile bottom card — rendered outside the overlay so it's not trapped in its stacking context -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="spotlight-mobile-card {mobileCardAtTop ? 'spotlight-mobile-card-top' : ''}"
      role="dialog"
      aria-label="Onboarding tour"
      tabindex="-1"
      ontouchstart={handleTouchStart}
      ontouchend={handleTouchEnd}
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
          {#if currentStepIndex > 0}
            <Button variant="ghost" size="sm" onclick={prev}>
              Back
            </Button>
          {/if}
          <Button variant="ghost" size="sm" onclick={() => finish()}>
            Skip
          </Button>
          <Button size="sm" onclick={next}>
            {isLastStep ? "Done" : "Next"}
          </Button>
        </div>
      </div>
    </div>
  {/if}
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

  .spotlight-mobile-card {
    position: fixed;
    bottom: 0;
    left: 0;
    right: 0;
    z-index: 10001;
    background: var(--popover);
    color: var(--popover-foreground);
    border-top: 1px solid var(--border);
    border-radius: 0.75rem 0.75rem 0 0;
    padding: 1.25rem 1rem calc(env(safe-area-inset-bottom) + 1rem);
    box-shadow: 0 -4px 20px rgba(0, 0, 0, 0.15);
    animation: slideUp 0.25s ease-out;
    transition: bottom 0.3s ease-in-out, top 0.3s ease-in-out;
  }

  .spotlight-mobile-card-top {
    bottom: auto;
    top: 0;
    border-top: none;
    border-bottom: 1px solid var(--border);
    border-radius: 0 0 0.75rem 0.75rem;
    padding: calc(env(safe-area-inset-top) + 1rem) 1rem 1.25rem;
    box-shadow: 0 4px 20px rgba(0, 0, 0, 0.15);
    animation: slideDown 0.25s ease-out;
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

  @keyframes slideUp {
    from {
      transform: translateY(100%);
    }
    to {
      transform: translateY(0);
    }
  }

  @keyframes slideDown {
    from {
      transform: translateY(-100%);
    }
    to {
      transform: translateY(0);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .spotlight-backdrop,
    .spotlight-tooltip,
    .spotlight-mobile-card,
    .spotlight-mobile-card-top {
      transition: none;
      animation: none;
    }
  }
</style>
