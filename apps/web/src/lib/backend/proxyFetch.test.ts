import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn();
const getCredentialMock = vi.fn();
const isTauriMock = vi.fn();

vi.mock("./interface", () => ({
  isTauri: () => isTauriMock(),
}));

vi.mock("$lib/credentials", () => ({
  getCredential: (...args: unknown[]) => getCredentialMock(...args),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { proxyFetch } from "./proxyFetch";

describe("proxyFetch", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("uses browser fetch with credentials for app requests and omits them for cdn requests", async () => {
    isTauriMock.mockReturnValue(false);
    const fetchMock = vi.fn().mockResolvedValue(new Response("ok", { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);

    await proxyFetch("https://app.diaryx.org/api/health");
    await proxyFetch("https://app.diaryx.org/cdn/plugin.wasm");

    expect(fetchMock.mock.calls[0]?.[1]).toMatchObject({ credentials: "include" });
    expect(fetchMock.mock.calls[1]?.[1]).toMatchObject({ credentials: "omit" });
  });

  it("routes tauri requests through invoke and injects auth when available", async () => {
    isTauriMock.mockReturnValue(true);
    getCredentialMock.mockResolvedValue("secret-token");
    invokeMock.mockResolvedValue({
      status: 200,
      status_text: "OK",
      headers: { "content-type": "application/json" },
      body_base64: btoa('{"ok":true}'),
    });

    const response = await proxyFetch("https://app.diaryx.org/api/test", {
      method: "post",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ hello: "world" }),
      timeout_ms: 1234,
    });

    expect(invokeMock).toHaveBeenCalledWith(
      "proxy_fetch",
      expect.objectContaining({
        url: "https://app.diaryx.org/api/test",
        method: "POST",
        timeoutMs: 1234,
        headers: expect.objectContaining({
          Authorization: "Bearer secret-token",
          "content-type": "application/json",
        }),
      }),
    );
    await expect(response.json()).resolves.toEqual({ ok: true });
  });

  it("respects explicit credentials from caller", async () => {
    isTauriMock.mockReturnValue(false);
    const fetchMock = vi.fn().mockResolvedValue(new Response("ok", { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);

    // Caller explicitly passes "omit" for a non-CDN URL — proxyFetch should honour it.
    await proxyFetch("https://unpkg.com/some-package/file.wasm", { credentials: "omit" });

    expect(fetchMock.mock.calls[0]?.[1]).toMatchObject({ credentials: "omit" });
  });

  it("returns a null-body response for null-body statuses", async () => {
    isTauriMock.mockReturnValue(true);
    getCredentialMock.mockResolvedValue(null);
    invokeMock.mockResolvedValue({
      status: 204,
      status_text: "No Content",
      headers: {},
      body_base64: "",
    });

    const response = await proxyFetch("https://app.diaryx.org/api/test");

    expect(response.status).toBe(204);
    expect(response.body).toBeNull();
  });
});
