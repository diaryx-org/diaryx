export function bubbleMenuHasRelevantFocus(
  bubbleMenuElement: HTMLElement | undefined,
  activeElement: Element | null,
  editorHasFocus: boolean,
): boolean {
  if (editorHasFocus) return true;
  if (!bubbleMenuElement || !activeElement) return false;
  return bubbleMenuElement.contains(activeElement);
}

export function shouldKeepBubbleMenuVisible(args: {
  bubbleMenuElement: HTMLElement | undefined;
  activeElement: Element | null;
  editorHasFocus: boolean;
  linkPopoverOpen: boolean;
}): boolean {
  if (args.linkPopoverOpen) return true;
  return bubbleMenuHasRelevantFocus(
    args.bubbleMenuElement,
    args.activeElement,
    args.editorHasFocus,
  );
}
