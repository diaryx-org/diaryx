/**
 * Toast Service
 *
 * Centralized error and notification handling using svelte-sonner.
 * Provides a consistent API for showing toast notifications across the app.
 */

import { toast } from 'svelte-sonner';

/**
 * Show an error toast notification.
 * Use this instead of setting error state that hides UI elements.
 *
 * @param message - The error message to display
 * @param context - Optional context for debugging (logged to console)
 */
export function showError(message: string | Error, context?: string) {
  const errorMessage = message instanceof Error ? message.message : message;

  if (context) {
    console.error(`[${context}]`, message);
  } else {
    console.error(message);
  }

  toast.error(errorMessage, {
    description: context,
    duration: 5000,
  });
}

/**
 * Show a success toast notification.
 */
export function showSuccess(message: string, description?: string) {
  toast.success(message, {
    description,
    duration: 3000,
  });
}

/**
 * Show a warning toast notification.
 */
export function showWarning(message: string, description?: string) {
  toast.warning(message, {
    description,
    duration: 4000,
  });
}

/**
 * Show an info toast notification.
 */
export function showInfo(message: string, description?: string) {
  toast.info(message, {
    description,
    duration: 3000,
  });
}

/**
 * Show a loading toast that can be updated later.
 * Returns a function to dismiss or update the toast.
 *
 * @param message - The loading message
 * @returns An object with methods to update or dismiss the toast
 */
export function showLoading(message: string) {
  const id = toast.loading(message);

  return {
    success: (successMessage: string) => {
      toast.success(successMessage, { id, duration: 3000 });
    },
    error: (errorMessage: string) => {
      toast.error(errorMessage, { id, duration: 5000 });
    },
    dismiss: () => {
      toast.dismiss(id);
    },
    update: (newMessage: string) => {
      toast.loading(newMessage, { id });
    },
  };
}

/**
 * Handle an error from an async operation.
 * Extracts the message and shows an error toast.
 *
 * @param error - The error (can be Error, string, or unknown)
 * @param context - Context for debugging
 */
export function handleError(error: unknown, context: string) {
  const message = error instanceof Error ? error.message : String(error);
  showError(message, context);
}
