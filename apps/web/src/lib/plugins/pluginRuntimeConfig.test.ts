import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("$lib/backend/interface", () => ({
  isTauri: () => false,
}));

import {
  getRuntimePluginCommandParams,
  mergeRuntimePluginConfig,
} from "./pluginRuntimeConfig";

describe("pluginRuntimeConfig", () => {
  let originalWindow: typeof globalThis.window | undefined;

  beforeEach(() => {
    originalWindow = (globalThis as { window?: typeof globalThis.window }).window;
    (globalThis as { window?: { location: { origin: string } } }).window = {
      location: {
        origin: "https://app.example",
      },
    };
  });

  afterEach(() => {
    (globalThis as { window?: typeof globalThis.window }).window = originalWindow;
  });

  it("leaves unrelated plugin config unchanged", () => {
    const config = {
      server_url: "https://sync.example",
    };

    expect(mergeRuntimePluginConfig("diaryx.sync", config)).toEqual(config);
  });

  it("preserves an explicit Google Drive client_id", () => {
    const merged = mergeRuntimePluginConfig("diaryx.storage.gdrive", {
      client_id: "configured-client",
    });

    expect(merged.client_id).toBe("configured-client");
  });

  it("preserves an explicit GitHub client_id", () => {
    const merged = mergeRuntimePluginConfig("diaryx.github", {
      client_id: "configured-client",
    });

    expect(merged.client_id).toBe("configured-client");
  });

  it("builds PKCE OAuth params for Google Drive begin-oauth", async () => {
    const params = await getRuntimePluginCommandParams(
      "diaryx.storage.gdrive",
      "BeginOAuth",
      {
        client_id: "configured-client",
      },
    );

    expect(params.client_id).toBe("configured-client");
    expect(params.redirect_uri).toBe("https://app.example/oauth/callback");
    expect(params.redirect_uri_prefix).toBe("https://app.example/oauth/callback");
    expect(typeof params.code_verifier).toBe("string");
    expect(typeof params.code_challenge).toBe("string");
    expect((params.code_verifier as string).length).toBeGreaterThan(10);
    expect((params.code_challenge as string).length).toBeGreaterThan(10);
  });

  it("returns no runtime params for unrelated commands", async () => {
    await expect(
      getRuntimePluginCommandParams("diaryx.storage.gdrive", "ListFiles", {
        client_id: "configured-client",
      }),
    ).resolves.toEqual({});
  });

  it("builds PKCE OAuth params for GitHub begin-oauth", async () => {
    const params = await getRuntimePluginCommandParams(
      "diaryx.github",
      "BeginOAuth",
      {
        client_id: "configured-client",
      },
    );

    expect(params.client_id).toBe("configured-client");
    expect(params.redirect_uri).toBe("https://app.example/oauth/callback");
    expect(params.redirect_uri_prefix).toBe("https://app.example/oauth/callback");
    expect(typeof params.code_verifier).toBe("string");
    expect(typeof params.code_challenge).toBe("string");
  });
});
