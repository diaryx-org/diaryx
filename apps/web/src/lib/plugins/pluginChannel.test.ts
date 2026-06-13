import { afterEach, describe, expect, it } from "vitest";

import {
  clearPluginChannel,
  getPluginChannel,
  setPluginChannel,
} from "./pluginChannel.svelte";

afterEach(() => {
  // Reset any channel state touched by tests so cases stay independent.
  for (const id of ["a.b", "c.d", "e.f"]) clearPluginChannel(id);
  localStorage.clear();
});

describe("pluginChannel", () => {
  it("defaults to the stable channel", () => {
    expect(getPluginChannel("a.b")).toBe("stable");
  });

  it("opts a plugin into the dev channel and persists it", () => {
    setPluginChannel("a.b", "dev");
    expect(getPluginChannel("a.b")).toBe("dev");

    const stored = JSON.parse(localStorage.getItem("diaryx-plugin-channel") ?? "{}");
    expect(stored["a.b"]).toBe("dev");
  });

  it("clears the entry when set back to stable (the default)", () => {
    setPluginChannel("c.d", "dev");
    setPluginChannel("c.d", "stable");

    expect(getPluginChannel("c.d")).toBe("stable");
    const stored = JSON.parse(localStorage.getItem("diaryx-plugin-channel") ?? "{}");
    expect(stored["c.d"]).toBeUndefined();
  });

  it("clearPluginChannel removes an opt-in", () => {
    setPluginChannel("e.f", "dev");
    clearPluginChannel("e.f");
    expect(getPluginChannel("e.f")).toBe("stable");
  });
});
