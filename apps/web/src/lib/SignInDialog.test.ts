import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { beforeEach, describe, expect, it, vi } from "vitest";

const authState = {
  isAuthenticated: false,
  user: null as null | { email: string },
};

const requestMagicLinkMock = vi.fn();
const verifyMagicLinkMock = vi.fn();
const reconnectServerMock = vi.fn();
const initAuthMock = vi.fn();
const setServerUrlMock = vi.fn();
const logoutMock = vi.fn();
const authenticateWithPasskeyMock = vi.fn();
const normalizeServerUrlMock = vi.fn(async (url: string) => url);
const proxyFetchMock = vi.fn();
const getBackendMock = vi.fn(async () => ({}));
const createApiMock = vi.fn(() => ({ normalizeServerUrl: normalizeServerUrlMock }));
const isPasskeySupportedMock = vi.fn(async () => false);

vi.mock("$lib/backend/proxyFetch", () => ({
  proxyFetch: (...args: unknown[]) => proxyFetchMock(...args),
}));

vi.mock("$lib/components/ui/button", async () => ({
  Button: (await import("./test/ButtonStub.svelte")).default,
}));

vi.mock("$lib/components/ui/input", async () => ({
  Input: (await import("./test/InputStub.svelte")).default,
}));

vi.mock("$lib/components/ui/label", async () => ({
  Label: (await import("./test/LabelStub.svelte")).default,
}));

vi.mock("$lib/components/ui/separator", async () => ({
  Separator: (await import("./test/SeparatorStub.svelte")).default,
}));

vi.mock("$lib/components/ui/dialog", async () => {
  const S = (await import("./test/PassthroughStub.svelte")).default;
  return { Root: S, Content: S, Header: S, Title: S };
});

vi.mock("$lib/storage/localWorkspaceRegistry.svelte", () => ({
  getLocalWorkspaces: () => [],
  getWorkspaceProviderLinks: () => [],
}));

vi.mock("$lib/SignOutDialog.svelte", async () => ({
  default: (await import("./test/SignOutDialogStub.svelte")).default,
}));

vi.mock("$lib/components/VerificationCodeInput.svelte", async () => ({
  default: (await import("./test/VerificationCodeInputStub.svelte")).default,
}));

vi.mock("$lib/auth", () => ({
  getAuthState: () => authState,
  logout: (...args: unknown[]) => logoutMock(...args),
  initAuth: () => initAuthMock(),
  setServerUrl: (...args: unknown[]) => setServerUrlMock(...args),
  requestMagicLink: (...args: unknown[]) => requestMagicLinkMock(...args),
  verifyMagicLink: (...args: unknown[]) => verifyMagicLinkMock(...args),
  reconnectServer: (...args: unknown[]) => reconnectServerMock(...args),
  listUserWorkspaceNamespaces: () => Promise.resolve([]),
}));

vi.mock("$lib/auth/authStore.svelte", () => ({
  authenticateWithPasskey: (...args: unknown[]) => authenticateWithPasskeyMock(...args),
}));

vi.mock("$lib/auth/webauthnUtils", () => ({
  isPasskeySupported: () => isPasskeySupportedMock(),
}));

vi.mock("$lib/backend/interface", () => ({
  isTauri: () => false,
}));

vi.mock("$lib/backend", () => ({
  getBackend: () => getBackendMock(),
  createApi: () => createApiMock(),
}));

vi.mock("@/models/stores/collaborationStore.svelte", () => ({
  collaborationStore: { serverOffline: false },
}));

import SignInDialog from "./SignInDialog.svelte";

describe("SignInDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    authState.isAuthenticated = false;
    authState.user = null;
    normalizeServerUrlMock.mockResolvedValue("https://app.diaryx.org/api");
    proxyFetchMock.mockResolvedValue(new Response("ok", { status: 200 }));
  });

  it("sends a magic link and transitions into verification state", async () => {
    requestMagicLinkMock.mockResolvedValue({ success: true, devLink: null, devCode: null });

    render(SignInDialog, { open: true });

    await fireEvent.input(screen.getByLabelText("Email"), {
      target: { value: "user@example.com" },
    });
    await fireEvent.click(screen.getByRole("button", { name: "Send Sign-in Link" }));

    await waitFor(() => {
      expect(requestMagicLinkMock).toHaveBeenCalledWith("user@example.com");
    });

    expect(proxyFetchMock).toHaveBeenCalledWith("https://app.diaryx.org/api/health", {
      method: "GET",
      timeout_ms: 5000,
    });
    expect(screen.getByText(/Check your email at/i)).toBeInTheDocument();
  });

  it("renders the authenticated actions and calls account settings callbacks", async () => {
    authState.isAuthenticated = true;
    authState.user = { email: "user@example.com" };

    const onOpenAccountSettings = vi.fn();

    render(SignInDialog, {
      open: true,
      onOpenAccountSettings,
    });

    await fireEvent.click(screen.getByRole("button", { name: "Account Settings" }));

    expect(onOpenAccountSettings).toHaveBeenCalledTimes(1);
    expect(screen.getByText("user@example.com")).toBeInTheDocument();
  });
});
