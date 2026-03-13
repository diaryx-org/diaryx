import { getCurrentWindow } from "@tauri-apps/api/window";
import { isTauri } from "$lib/backend/interface";
import { isIOS } from "$lib/hooks/useMobile.svelte";

const WINDOW_DRAG_EXCLUDE_SELECTOR = [
  "button",
  "a",
  "input",
  "textarea",
  "select",
  "option",
  "label",
  "summary",
  "[role='button']",
  "[role='link']",
  "[role='menuitem']",
  "[role='checkbox']",
  "[role='radio']",
  "[role='switch']",
  "[contenteditable='']",
  "[contenteditable='true']",
  "[contenteditable='plaintext-only']",
  "[data-window-drag-exclude]",
].join(", ");

export function shouldStartWindowDrag(event: MouseEvent): boolean {
  if (!isTauri() || isIOS()) return false;
  if (event.defaultPrevented) return false;
  if (event.button !== 0 || (event.buttons !== 0 && event.buttons !== 1)) return false;
  if (event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) return false;

  const target = event.target;
  if (!(target instanceof Element)) return true;

  return target.closest(WINDOW_DRAG_EXCLUDE_SELECTOR) === null;
}

export async function maybeStartWindowDrag(event: MouseEvent): Promise<void> {
  if (!shouldStartWindowDrag(event)) return;

  try {
    await getCurrentWindow().startDragging();
  } catch (error) {
    console.warn("[windowDrag] Failed to start window drag:", error);
  }
}
