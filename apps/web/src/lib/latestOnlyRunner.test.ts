import { describe, expect, it } from "vitest";

import { createLatestOnlyRunner } from "./latestOnlyRunner";

describe("createLatestOnlyRunner", () => {
  it("runs a single request immediately", async () => {
    const seen: string[] = [];
    const run = createLatestOnlyRunner(async (value: string) => {
      seen.push(value);
    });

    await run("alpha");

    expect(seen).toEqual(["alpha"]);
  });

  it("drops intermediate queued values and keeps the latest", async () => {
    const seen: string[] = [];
    const deferred: { resolve: () => void } = { resolve: () => {} };

    const run = createLatestOnlyRunner(async (value: string) => {
      seen.push(value);
      if (value === "alpha") {
        await new Promise<void>((resolve) => {
          deferred.resolve = resolve;
        });
      }
    });

    const first = run("alpha");
    const second = run("beta");
    const third = run("gamma");

    deferred.resolve();

    await Promise.all([first, second, third]);

    expect(seen).toEqual(["alpha", "gamma"]);
  });
});
