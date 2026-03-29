import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  isTauri: vi.fn(),
  getBackendSync: vi.fn(),
}));

vi.mock("$lib/backend", () => ({
  isTauri: mocks.isTauri,
  getBackendSync: mocks.getBackendSync,
}));

import {
  BUILTIN_ICLOUD_PROVIDER_ID,
  getBuiltinProvider,
  isAppleTauriRuntime,
  getBuiltinWorkspaceProviders,
  getProviderCapabilities,
  getProviderDisplayLabel,
  getProviderUnavailableReason,
  isBuiltinProvider,
  isProviderAvailableHere,
} from "./builtinProviders";

describe("builtinProviders", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe("BUILTIN_ICLOUD_PROVIDER_ID", () => {
    it("is builtin.icloud", () => {
      expect(BUILTIN_ICLOUD_PROVIDER_ID).toBe("builtin.icloud");
    });
  });

  describe("isAppleTauriRuntime", () => {
    it("returns false when not in Tauri", () => {
      mocks.isTauri.mockReturnValue(false);
      expect(isAppleTauriRuntime()).toBe(false);
    });

    it("returns true when in Tauri with Apple build", () => {
      mocks.isTauri.mockReturnValue(true);
      mocks.getBackendSync.mockReturnValue({
        getAppPaths: () => ({ is_apple_build: true }),
      });
      expect(isAppleTauriRuntime()).toBe(true);
    });

    it("returns false when in Tauri without Apple build", () => {
      mocks.isTauri.mockReturnValue(true);
      mocks.getBackendSync.mockReturnValue({
        getAppPaths: () => ({ is_apple_build: false }),
      });
      expect(isAppleTauriRuntime()).toBe(false);
    });

    it("returns false when getAppPaths returns null", () => {
      mocks.isTauri.mockReturnValue(true);
      mocks.getBackendSync.mockReturnValue({
        getAppPaths: () => null,
      });
      expect(isAppleTauriRuntime()).toBe(false);
    });

    it("returns false when getBackendSync throws", () => {
      mocks.isTauri.mockReturnValue(true);
      mocks.getBackendSync.mockImplementation(() => {
        throw new Error("no backend");
      });
      expect(isAppleTauriRuntime()).toBe(false);
    });
  });

  describe("getBuiltinWorkspaceProviders", () => {
    it("returns empty array when not Apple Tauri runtime", () => {
      mocks.isTauri.mockReturnValue(false);
      expect(getBuiltinWorkspaceProviders()).toEqual([]);
    });

    it("returns iCloud provider when on Apple Tauri runtime", () => {
      mocks.isTauri.mockReturnValue(true);
      mocks.getBackendSync.mockReturnValue({
        getAppPaths: () => ({ is_apple_build: true }),
      });

      const providers = getBuiltinWorkspaceProviders();
      expect(providers).toHaveLength(1);
      const provider = providers[0]!;
      const caps = provider.capabilities!;
      expect(provider.pluginId).toBe("builtin.icloud");
      expect(provider.contribution.label).toBe("iCloud Drive");
      expect(provider.source).toBe("builtin");
      expect(caps.available).toBe(true);
      expect(caps.canLink).toBe(true);
      expect(caps.canDownload).toBe(true);
      expect(caps.canListRemote).toBe(true);
    });
  });

  describe("isBuiltinProvider", () => {
    it("returns true for builtin. prefixed ids", () => {
      expect(isBuiltinProvider("builtin.icloud")).toBe(true);
      expect(isBuiltinProvider("builtin.anything")).toBe(true);
    });

    it("returns false for non-builtin ids", () => {
      expect(isBuiltinProvider("diaryx.sync")).toBe(false);
      expect(isBuiltinProvider("icloud")).toBe(false);
      expect(isBuiltinProvider("")).toBe(false);
    });
  });

  describe("getBuiltinProvider", () => {
    it("returns null when not Apple Tauri runtime", () => {
      mocks.isTauri.mockReturnValue(false);
      expect(getBuiltinProvider("builtin.icloud")).toBeNull();
    });

    it("returns the iCloud provider when on Apple Tauri runtime", () => {
      mocks.isTauri.mockReturnValue(true);
      mocks.getBackendSync.mockReturnValue({
        getAppPaths: () => ({ is_apple_build: true }),
      });

      const provider = getBuiltinProvider("builtin.icloud");
      expect(provider).not.toBeNull();
      expect(provider!.pluginId).toBe("builtin.icloud");
    });

    it("returns null for unknown builtin provider ids", () => {
      mocks.isTauri.mockReturnValue(true);
      mocks.getBackendSync.mockReturnValue({
        getAppPaths: () => ({ is_apple_build: true }),
      });

      expect(getBuiltinProvider("builtin.unknown")).toBeNull();
    });
  });

  describe("provider availability helpers", () => {
    it("reports iCloud as unavailable outside Apple Tauri", () => {
      mocks.isTauri.mockReturnValue(false);

      expect(getProviderDisplayLabel("builtin.icloud")).toBe("iCloud Drive");
      expect(getProviderCapabilities("builtin.icloud")).toMatchObject({
        available: false,
        canLink: false,
        canDownload: false,
        canListRemote: false,
      });
      expect(isProviderAvailableHere("builtin.icloud")).toBe(false);
      expect(getProviderUnavailableReason("builtin.icloud")).toContain("Apple devices");
    });

    it("reports iCloud as available on Apple Tauri", () => {
      mocks.isTauri.mockReturnValue(true);
      mocks.getBackendSync.mockReturnValue({
        getAppPaths: () => ({ is_apple_build: true }),
      });

      expect(getProviderCapabilities("builtin.icloud")).toMatchObject({
        available: true,
        canLink: true,
        canDownload: true,
        canListRemote: true,
      });
      expect(isProviderAvailableHere("builtin.icloud")).toBe(true);
      expect(getProviderUnavailableReason("builtin.icloud")).toBeNull();
    });

    it("treats unknown providers as available by default", () => {
      expect(getProviderCapabilities("diaryx.sync")).toBeNull();
      expect(isProviderAvailableHere("diaryx.sync")).toBe(true);
      expect(getProviderUnavailableReason("diaryx.sync")).toBeNull();
    });
  });
});
