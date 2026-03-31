/**
 * Coalesce rapid async requests so only the latest queued value is processed
 * after the current task finishes. Intermediate queued values are dropped.
 */
export function createLatestOnlyRunner<T>(
  worker: (value: T) => Promise<void>,
): (value: T) => Promise<void> {
  let pendingValue: T | undefined;
  let drainPromise: Promise<void> | null = null;

  async function drain(): Promise<void> {
    while (pendingValue !== undefined) {
      const value = pendingValue;
      pendingValue = undefined;
      await worker(value);
    }
  }

  return async (value: T): Promise<void> => {
    pendingValue = value;
    if (!drainPromise) {
      drainPromise = (async () => {
        try {
          await drain();
        } finally {
          drainPromise = null;
        }
      })();
    }

    await drainPromise;
  };
}
