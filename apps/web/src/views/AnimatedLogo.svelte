<script lang="ts">
  /**
   * AnimatedLogo — three-stage clip-path reveal of the pen nib SVG,
   * then crossfade to the full-colour PNG icon.
   * Click to bounce, drag to stretch.
   */

  import { tick } from 'svelte';

  interface Props {
    /** px size of the logo box */
    size?: number;
  }

  let { size = 64 }: Props = $props();

  let wrapEl: HTMLDivElement;

  let bouncing = $state(false);

  // Drag state
  let pointerIsDown = false;
  let dragging = $state(false);
  let stretchX = $state(1);
  let stretchY = $state(1);
  let originX = $state('50%');
  let originY = $state('50%');
  let snapping = $state(false);
  let dragOrigin = { x: 0, y: 0 };
  let pointerId = -1;

  const DRAG_THRESHOLD = 5;

  async function triggerBounce() {
    bouncing = false;
    await tick();
    bouncing = true;
  }

  function handlePointerDown(e: PointerEvent) {
    if (e.button !== 0) return;
    pointerIsDown = true;
    dragOrigin = { x: e.clientX, y: e.clientY };
    pointerId = e.pointerId;
  }

  function handlePointerMove(e: PointerEvent) {
    if (!pointerIsDown) return;
    const dx = e.clientX - dragOrigin.x;
    const dy = e.clientY - dragOrigin.y;

    if (!dragging) {
      if (Math.abs(dx) > DRAG_THRESHOLD || Math.abs(dy) > DRAG_THRESHOLD) {
        dragging = true;
        snapping = false;
        wrapEl.setPointerCapture(pointerId);
      } else {
        return;
      }
    }

    const maxStretch = 0.4;
    const absDx = Math.abs(dx);
    const absDy = Math.abs(dy);

    // Always stretch outward (scale > 1), origin on the opposite side
    stretchX = 1 + Math.tanh(absDx / (size * 1.5)) * maxStretch;
    stretchY = 1 + Math.tanh(absDy / (size * 1.5)) * maxStretch;
    originX = dx >= 0 ? '0%' : '100%';
    originY = dy >= 0 ? '0%' : '100%';
  }

  function handlePointerUp() {
    if (!pointerIsDown) return;
    pointerIsDown = false;
    if (dragging) {
      dragging = false;
      snapping = true;
      stretchX = 1;
      stretchY = 1;
    } else {
      triggerBounce();
    }
  }

  function handleTransitionEnd() {
    snapping = false;
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  bind:this={wrapEl}
  class="logo-wrap"
  class:snapping
  style="width:{size}px;height:{size}px;transform:scale({stretchX},{stretchY});transform-origin:{originX} {originY}"
  onpointerdown={handlePointerDown}
  onpointermove={handlePointerMove}
  onpointerup={handlePointerUp}
  onpointercancel={handlePointerUp}
  ontransitionend={handleTransitionEnd}
>
  <div
    class="bounce-inner"
    class:bounce={bouncing}
    onanimationend={() => bouncing = false}
  >
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <img
      src="/diaryx_icon.svg"
      alt=""
      class="nib-svg draw-nib"
      ondragstart={e => e.preventDefault()}
    />
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <img
      src="/icon.png"
      alt="Diaryx"
      class="icon-png reveal-png"
      ondragstart={e => e.preventDefault()}
    />
  </div>
</div>

<style>
  .logo-wrap {
    position: relative;
    cursor: pointer;
    touch-action: none;
    user-select: none;
  }

  .logo-wrap.snapping {
    transition: transform 0.5s cubic-bezier(0.34, 1.56, 0.64, 1);
  }

  .bounce-inner {
    width: 100%;
    height: 100%;
    position: relative;
  }

  .bounce {
    animation: bounce 0.5s ease;
  }

  @keyframes bounce {
    0%   { transform: scale(1); }
    30%  { transform: scale(0.85); }
    50%  { transform: scale(1.15); }
    70%  { transform: scale(0.95); }
    100% { transform: scale(1); }
  }

  .icon-png {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: contain;
  }

  .nib-svg {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: 40%;
    height: auto;
    object-fit: contain;
  }

  .draw-nib {
    clip-path: polygon(0% 0%, 100% 0%, 100% 0%, 50% 0%, 50% 0%, 0% 0%);
    animation: drawNib 1.6s ease-in-out forwards,
               logoFadeOut 0.4s ease-out 1.8s forwards;
  }

  .reveal-png {
    opacity: 0;
    animation: logoFadeIn 0.4s ease-out 1.8s forwards;
  }

  /*
   * 6-point polygon so each half reveals top-to-bottom independently.
   *
   *   0%  → all collapsed along top edge
   *  33%  → left half drops to 85% (L-shape)
   *  66%  → right half also drops to 85% (full width at 85%)
   * 100%  → bottom drops to 100%
   */
  @keyframes drawNib {
    0%   { clip-path: polygon(0% 0%, 100% 0%, 100% 0%,  50% 0%,  50% 0%,  0% 0%);  }
    33%  { clip-path: polygon(0% 0%, 100% 0%, 100% 0%,  50% 0%,  50% 85%, 0% 85%); }
    66%  { clip-path: polygon(0% 0%, 100% 0%, 100% 85%, 50% 85%, 50% 85%, 0% 85%); }
    100% { clip-path: polygon(0% 0%, 100% 0%, 100% 100%, 50% 100%, 50% 100%, 0% 100%); }
  }

  @keyframes logoFadeOut {
    from { opacity: 1; }
    to   { opacity: 0; }
  }

  @keyframes logoFadeIn {
    from { opacity: 0; }
    to   { opacity: 1; }
  }

  @media (prefers-reduced-motion: reduce) {
    .draw-nib {
      animation: none;
      clip-path: none;
      opacity: 0;
    }
    .reveal-png {
      animation: none;
      opacity: 1;
    }
    .bounce {
      animation: none;
    }
    .logo-wrap.snapping {
      transition: none;
    }
  }
</style>
