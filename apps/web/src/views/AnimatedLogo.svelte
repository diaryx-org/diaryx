<script lang="ts">
  /**
   * AnimatedLogo — three-stage clip-path reveal of the pen nib SVG,
   * then crossfade to the full-colour PNG icon.
   */

  interface Props {
    /** px size of the logo box */
    size?: number;
  }

  let { size = 64 }: Props = $props();
</script>

<div class="logo-wrap" style="width:{size}px;height:{size}px">
  <img
    src="/diaryx_icon.svg"
    alt=""
    class="nib-svg draw-nib"
  />
  <img
    src="/icon.png"
    alt="Diaryx"
    class="icon-png reveal-png"
  />
</div>

<style>
  .logo-wrap {
    position: relative;
  }

  /* PNG fills the whole box */
  .icon-png {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: contain;
  }

  /*
   * SVG is the raw nib (portrait 867×1535).
   * Scale and position it to overlay the nib inside the PNG.
   * The nib in the PNG sits roughly 50% wide, 70% tall, centered.
   */
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
    clip-path: polygon(0% 0%, 50% 0%, 50% 0%, 0% 0%);
    animation: drawNib 1.6s ease-in-out forwards,
               logoFadeOut 0.4s ease-out 1.8s forwards;
  }

  .reveal-png {
    opacity: 0;
    animation: logoFadeIn 0.4s ease-out 1.8s forwards;
  }

  /*
   * Three-stage reveal:
   *   0%  → collapsed at top-left
   *  33%  → left half, top 85%
   *  66%  → full width, top 85%
   * 100%  → full
   */
  @keyframes drawNib {
    0%   { clip-path: polygon(0% 0%, 50% 0%, 50% 0%,   0% 0%);   }
    33%  { clip-path: polygon(0% 0%, 50% 0%, 50% 85%,  0% 85%);  }
    66%  { clip-path: polygon(0% 0%, 100% 0%, 100% 85%, 0% 85%); }
    100% { clip-path: polygon(0% 0%, 100% 0%, 100% 100%, 0% 100%); }
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
  }
</style>
