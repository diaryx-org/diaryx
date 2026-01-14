/**
 * Mobile detection and virtual keyboard tracking utilities.
 * Uses Svelte 5 runes for reactive state management.
 */

/**
 * Creates reactive state for mobile/touch detection and virtual keyboard tracking.
 *
 * @returns Object with reactive properties:
 *   - isMobile: true if screen width < 768px
 *   - isTouchDevice: true if device supports touch
 *   - keyboardVisible: true if virtual keyboard is likely visible
 *   - keyboardHeight: estimated height of virtual keyboard in pixels
 *   - viewportOffsetTop: iOS visual viewport scroll offset (for fixed element positioning)
 *   - viewportHeight: current visual viewport height (accounts for keyboard)
 */
export function createMobileState() {
  let isMobile = $state(false);
  let isTouchDevice = $state(false);
  let keyboardVisible = $state(false);
  let keyboardHeight = $state(0);
  let viewportOffsetTop = $state(0);
  let viewportHeight = $state(typeof window !== 'undefined' ? window.innerHeight : 0);

  if (typeof window !== 'undefined') {
    // Initial detection
    isTouchDevice = 'ontouchstart' in window || navigator.maxTouchPoints > 0;
    isMobile = window.innerWidth < 768;

    // Listen for resize to update mobile state
    const handleResize = () => {
      isMobile = window.innerWidth < 768;
    };
    window.addEventListener('resize', handleResize);

    // Virtual keyboard detection via visualViewport API
    // This is the most reliable method for detecting keyboard on iOS/Android
    if (window.visualViewport) {
      const handleViewportResize = () => {
        const viewport = window.visualViewport!;
        // Calculate difference between window height and visual viewport height
        // When keyboard is open, visualViewport.height is reduced
        const heightDiff = window.innerHeight - viewport.height;

        // Always update viewport height for positioning calculations
        viewportHeight = viewport.height;

        // Use different thresholds for opening vs closing (hysteresis)
        // This prevents flickering during scrolling
        const openThreshold = 150;  // Height diff to consider keyboard "opened"
        const closeThreshold = 100; // Height diff to consider keyboard "closed"

        if (keyboardVisible) {
          // Keyboard is currently visible - require larger change to close
          if (heightDiff < closeThreshold) {
            keyboardVisible = false;
            keyboardHeight = 0;
          } else {
            // Update height but keep visible
            keyboardHeight = heightDiff;
          }
        } else {
          // Keyboard is currently hidden - check if it opened
          if (heightDiff > openThreshold) {
            keyboardVisible = true;
            keyboardHeight = heightDiff;
          }
        }
      };

      // Track viewport scroll offset for fixed positioning adjustment
      // On iOS, when scrolling with keyboard open, the viewport scrolls
      // and fixed elements need to account for this offset
      const handleViewportScroll = () => {
        const viewport = window.visualViewport!;
        viewportOffsetTop = viewport.offsetTop;
        // Also update height during scroll as it can change
        viewportHeight = viewport.height;
      };

      window.visualViewport.addEventListener('resize', handleViewportResize);
      window.visualViewport.addEventListener('scroll', handleViewportScroll);

      // Initial check
      handleViewportResize();
      handleViewportScroll();
    }
  }

  return {
    get isMobile() { return isMobile; },
    get isTouchDevice() { return isTouchDevice; },
    get keyboardVisible() { return keyboardVisible; },
    get keyboardHeight() { return keyboardHeight; },
    get viewportOffsetTop() { return viewportOffsetTop; },
    get viewportHeight() { return viewportHeight; },
  };
}

/**
 * Singleton instance for shared mobile state across components.
 * Use this when you want all components to share the same state.
 */
let sharedMobileState: ReturnType<typeof createMobileState> | null = null;

export function getMobileState() {
  if (typeof window === 'undefined') {
    // SSR fallback - return non-reactive defaults
    return {
      get isMobile() { return false; },
      get isTouchDevice() { return false; },
      get keyboardVisible() { return false; },
      get keyboardHeight() { return 0; },
      get viewportOffsetTop() { return 0; },
      get viewportHeight() { return 0; },
    };
  }

  if (!sharedMobileState) {
    sharedMobileState = createMobileState();
  }
  return sharedMobileState;
}

/**
 * Utility to check if the current device is likely iOS.
 * Useful for applying iOS-specific workarounds.
 */
export function isIOS(): boolean {
  if (typeof window === 'undefined') return false;

  return /iPad|iPhone|iPod/.test(navigator.userAgent) ||
    (navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1);
}

/**
 * Utility to check if the current device is likely Android.
 */
export function isAndroid(): boolean {
  if (typeof window === 'undefined') return false;

  return /Android/.test(navigator.userAgent);
}
