import { describe, expect, it, vi } from "vitest";

import { resolveHtmlPreviewMedia } from "./htmlPreviewMedia";

describe("resolveHtmlPreviewMedia", () => {
  it("rewrites img src and picture source srcset local paths", async () => {
    const resolver = vi.fn(async (rawPath: string) => `blob:${rawPath}`);
    const html = `
      <picture>
        <source media="(prefers-color-scheme: dark)" srcset="apps/web/public/icon-dark.png">
        <source media="(prefers-color-scheme: light)" srcset="apps/web/public/icon.png 1x, apps/web/public/icon@2x.png 2x">
        <img alt="Diaryx icon" src="apps/web/public/icon.png" width="128">
      </picture>
    `;

    const result = await resolveHtmlPreviewMedia(
      html,
      "Diaryx.md",
      {} as never,
      resolver as never,
    );

    expect(result).toContain('srcset="blob:apps/web/public/icon-dark.png"');
    expect(result).toContain(
      'srcset="blob:apps/web/public/icon.png 1x, blob:apps/web/public/icon@2x.png 2x"',
    );
    expect(result).toContain('src="blob:apps/web/public/icon.png"');
    expect(resolver).toHaveBeenCalledWith("apps/web/public/icon-dark.png", "Diaryx.md", {});
    expect(resolver).toHaveBeenCalledWith("apps/web/public/icon.png", "Diaryx.md", {});
    expect(resolver).toHaveBeenCalledWith("apps/web/public/icon@2x.png", "Diaryx.md", {});
  });

  it("leaves external srcset candidates unchanged", async () => {
    const resolver = vi.fn(async (rawPath: string) => rawPath);
    const html = '<source srcset="https://example.com/a.png 1x, https://example.com/b.png 2x">';

    const result = await resolveHtmlPreviewMedia(
      html,
      "Diaryx.md",
      {} as never,
      resolver as never,
    );

    expect(result).toBe(html);
  });
});
