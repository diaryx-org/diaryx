/**
 * Context Menu Hook
 *
 * Provides state management for context menu interactions that
 * automatically switches between desktop context menu and mobile
 * bottom sheet based on device type.
 */

import { getMobileState } from './useMobile.svelte';

/**
 * Creates state for managing context menu / bottom sheet interactions.
 *
 * Usage:
 * ```svelte
 * <script>
 *   const menuState = createContextMenuState();
 * </script>
 *
 * {#if menuState.useBottomSheet}
 *   <BottomSheet open={menuState.bottomSheetOpen} onClose={menuState.closeMenu}>
 *     <!-- menu items -->
 *   </BottomSheet>
 * {:else}
 *   <ContextMenu.Root>
 *     <!-- context menu -->
 *   </ContextMenu.Root>
 * {/if}
 * ```
 */
export function createContextMenuState<T = unknown>() {
  const mobile = getMobileState();

  let contextMenuOpen = $state(false);
  let bottomSheetOpen = $state(false);
  let targetData = $state<T | null>(null);

  /**
   * Open the menu for a target item.
   * Automatically chooses context menu or bottom sheet based on device.
   */
  function openMenu(data: T) {
    targetData = data;

    if (mobile.isMobile || mobile.isTouchDevice) {
      bottomSheetOpen = true;
      contextMenuOpen = false;
    } else {
      contextMenuOpen = true;
      bottomSheetOpen = false;
    }
  }

  /**
   * Close the menu.
   */
  function closeMenu() {
    contextMenuOpen = false;
    bottomSheetOpen = false;
    targetData = null;
  }

  /**
   * Handle right-click or long-press event.
   * Call this from the oncontextmenu handler.
   */
  function handleContextMenu(e: MouseEvent | TouchEvent, data: T) {
    e.preventDefault();
    openMenu(data);
  }

  return {
    /**
     * Whether to use bottom sheet instead of context menu.
     * True on mobile and touch devices.
     */
    get useBottomSheet() {
      return mobile.isMobile || mobile.isTouchDevice;
    },

    /**
     * Whether the context menu is currently open (desktop).
     */
    get contextMenuOpen() {
      return contextMenuOpen;
    },

    /**
     * Whether the bottom sheet is currently open (mobile).
     */
    get bottomSheetOpen() {
      return bottomSheetOpen;
    },

    /**
     * The data associated with the currently opened menu item.
     */
    get targetData() {
      return targetData;
    },

    /**
     * Whether any menu (context or bottom sheet) is currently open.
     */
    get isOpen() {
      return contextMenuOpen || bottomSheetOpen;
    },

    openMenu,
    closeMenu,
    handleContextMenu,
  };
}

/**
 * Type for the data passed to tree node context menus.
 */
export interface TreeNodeMenuData {
  path: string;
  name: string;
  hasChildren: boolean;
}
