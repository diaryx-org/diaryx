export type PluginHostUiToastVariant = "success" | "info" | "warning" | "error";
export type PluginHostUiDialogVariant = "default" | "destructive";

export type PluginHostAction = {
  type?: string;
  payload?: unknown;
};

export type PluginToastRequest = {
  message: string;
  description?: string;
  variant: PluginHostUiToastVariant;
};

export type PluginConfirmRequest = {
  title: string;
  description: string;
  confirmLabel: string;
  cancelLabel: string;
  variant: PluginHostUiDialogVariant;
};

export type PluginPromptRequest = PluginConfirmRequest & {
  value: string;
  placeholder: string;
};

export type PluginHostUiHandlers = {
  showToast: (request: PluginToastRequest) => void;
  confirm: (request: PluginConfirmRequest) => Promise<boolean>;
  prompt: (request: PluginPromptRequest) => Promise<string | null>;
};

const STANDARD_ACTIONS = new Set(["show-toast", "confirm", "prompt"]);

function readPayload(payload: unknown): Record<string, unknown> {
  return payload && typeof payload === "object" && !Array.isArray(payload)
    ? (payload as Record<string, unknown>)
    : {};
}

export function isStandardPluginHostUiAction(actionType: string | undefined): boolean {
  return !!actionType && STANDARD_ACTIONS.has(actionType);
}

export async function handleStandardPluginHostUiAction(
  action: PluginHostAction,
  handlers: PluginHostUiHandlers,
): Promise<unknown> {
  const actionType = action.type;
  const payload = readPayload(action.payload);

  switch (actionType) {
    case "show-toast": {
      const message = typeof payload.message === "string" ? payload.message.trim() : "";
      if (!message) {
        throw new Error("host-action show-toast requires payload.message");
      }

      handlers.showToast({
        message,
        description:
          typeof payload.description === "string" && payload.description.trim().length > 0
            ? payload.description.trim()
            : undefined,
        variant:
          payload.variant === "success" ||
          payload.variant === "info" ||
          payload.variant === "warning" ||
          payload.variant === "error"
            ? payload.variant
            : "info",
      });
      return { shown: true };
    }
    case "confirm":
      return handlers.confirm({
        title:
          typeof payload.title === "string" && payload.title.trim().length > 0
            ? payload.title.trim()
            : "Confirm",
        description: typeof payload.description === "string" ? payload.description : "",
        confirmLabel:
          typeof payload.confirmLabel === "string" && payload.confirmLabel.trim().length > 0
            ? payload.confirmLabel.trim()
            : "Confirm",
        cancelLabel:
          typeof payload.cancelLabel === "string" && payload.cancelLabel.trim().length > 0
            ? payload.cancelLabel.trim()
            : "Cancel",
        variant: payload.variant === "destructive" ? "destructive" : "default",
      });
    case "prompt":
      return handlers.prompt({
        title:
          typeof payload.title === "string" && payload.title.trim().length > 0
            ? payload.title.trim()
            : "Prompt",
        description: typeof payload.description === "string" ? payload.description : "",
        confirmLabel:
          typeof payload.confirmLabel === "string" && payload.confirmLabel.trim().length > 0
            ? payload.confirmLabel.trim()
            : "OK",
        cancelLabel:
          typeof payload.cancelLabel === "string" && payload.cancelLabel.trim().length > 0
            ? payload.cancelLabel.trim()
            : "Cancel",
        variant: payload.variant === "destructive" ? "destructive" : "default",
        value: typeof payload.value === "string" ? payload.value : "",
        placeholder: typeof payload.placeholder === "string" ? payload.placeholder : "",
      });
    default:
      throw new Error(`Unknown standard plugin UI host action: ${actionType ?? "undefined"}`);
  }
}
