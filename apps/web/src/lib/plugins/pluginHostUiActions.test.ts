import { describe, expect, it, vi } from "vitest";

import {
  handleStandardPluginHostUiAction,
  isStandardPluginHostUiAction,
} from "./pluginHostUiActions";

describe("pluginHostUiActions", () => {
  it("recognizes standard plugin UI actions", () => {
    expect(isStandardPluginHostUiAction("show-toast")).toBe(true);
    expect(isStandardPluginHostUiAction("confirm")).toBe(true);
    expect(isStandardPluginHostUiAction("prompt")).toBe(true);
    expect(isStandardPluginHostUiAction("open-entry")).toBe(false);
  });

  it("normalizes toast payloads", async () => {
    const showToast = vi.fn();
    const confirm = vi.fn(async () => false);
    const prompt = vi.fn(async () => null);

    await handleStandardPluginHostUiAction(
      { type: "show-toast", payload: { message: " Hello ", variant: "error" } },
      { showToast, confirm, prompt },
    );

    expect(showToast).toHaveBeenCalledWith({
      message: "Hello",
      description: undefined,
      variant: "error",
    });
  });

  it("provides defaults for prompt payloads", async () => {
    const result = "typed value";
    const prompt = vi.fn(async () => result);

    const response = await handleStandardPluginHostUiAction(
      { type: "prompt", payload: { title: " Name " } },
      {
        showToast: vi.fn(),
        confirm: vi.fn(async () => false),
        prompt,
      },
    );

    expect(prompt).toHaveBeenCalledWith({
      title: "Name",
      description: "",
      confirmLabel: "OK",
      cancelLabel: "Cancel",
      variant: "default",
      value: "",
      placeholder: "",
    });
    expect(response).toBe(result);
  });
});
