/**
 * Simple event emitter for Backend events.
 * Used by both WASM and Tauri backend implementations.
 */

import type { BackendEvent, BackendEventType, BackendEventListener } from './interface';

export class BackendEventEmitter {
  private listeners: Map<BackendEventType, Set<BackendEventListener>> = new Map();

  /**
   * Subscribe to a backend event.
   */
  on(event: BackendEventType, listener: BackendEventListener): void {
    if (!this.listeners.has(event)) {
      this.listeners.set(event, new Set());
    }
    this.listeners.get(event)!.add(listener);
  }

  /**
   * Unsubscribe from a backend event.
   */
  off(event: BackendEventType, listener: BackendEventListener): void {
    this.listeners.get(event)?.delete(listener);
  }

  /**
   * Emit an event to all subscribers.
   */
  emit(event: BackendEvent): void {
    const eventListeners = this.listeners.get(event.type);
    if (eventListeners) {
      for (const listener of eventListeners) {
        try {
          listener(event);
        } catch (e) {
          console.error(`[BackendEventEmitter] Error in ${event.type} listener:`, e);
        }
      }
    }
  }

  /**
   * Remove all listeners for all events.
   */
  removeAllListeners(): void {
    this.listeners.clear();
  }
}
