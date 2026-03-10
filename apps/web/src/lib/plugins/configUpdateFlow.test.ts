import { describe, expect, it, vi } from "vitest"

vi.mock("$lib/plugins/browserPluginManager.svelte", () => ({
  dispatchCommand: vi.fn(),
  getPlugin: vi.fn(() => null),
}))

import { applyPluginPermissionsPatch } from "./configUpdateFlow"

describe("configUpdateFlow", () => {
  it("merges AI http permissions without dropping existing hosts", () => {
    const nextPlugins = applyPluginPermissionsPatch(
      {
        "diaryx.ai": {
          permissions: {
            http_requests: {
              include: ["openrouter.ai"],
              exclude: ["blocked.example"],
            },
          },
        },
      },
      "diaryx.ai",
      {
        mode: "merge",
        permissions: {
          http_requests: {
            include: ["sync.example"],
            exclude: [],
          },
        },
      },
    )

    expect(nextPlugins?.["diaryx.ai"].permissions).toEqual({
      http_requests: {
        include: ["openrouter.ai", "sync.example"],
        exclude: ["blocked.example"],
      },
    })
  })

  it("replaces Daily file scopes while preserving unrelated permissions", () => {
    const nextPlugins = applyPluginPermissionsPatch(
      {
        "diaryx.daily": {
          permissions: {
            read_files: { include: ["all"], exclude: [] },
            edit_files: { include: ["all"], exclude: [] },
            create_files: { include: ["all"], exclude: [] },
            plugin_storage: { include: ["all"], exclude: [] },
          },
        },
      },
      "diaryx.daily",
      {
        mode: "replace",
        permissions: {
          read_files: { include: ["Daily", "README.md"], exclude: [] },
          edit_files: { include: ["Daily", "README.md"], exclude: [] },
          create_files: { include: ["Daily"], exclude: [] },
        },
      },
    )

    expect(nextPlugins?.["diaryx.daily"].permissions).toEqual({
      read_files: { include: ["Daily", "README.md"], exclude: [] },
      edit_files: { include: ["Daily", "README.md"], exclude: [] },
      create_files: { include: ["Daily"], exclude: [] },
      plugin_storage: { include: ["all"], exclude: [] },
    })
  })
})
