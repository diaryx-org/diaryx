/**
 * Auth Service - Magic link authentication for Diaryx sync server.
 */

import { proxyFetch } from "$lib/backend/proxyFetch";

export interface User {
  id: string;
  email: string;
}

export interface Workspace {
  id: string;
  name: string;
}

export interface Device {
  id: string;
  name: string | null;
  last_seen_at: string;
}

export interface VerifyResponse {
  success: boolean;
  token: string;
  user: User;
}

export interface MeResponse {
  user: User;
  workspaces: Workspace[];
  devices: Device[];
  workspace_limit: number;
  tier: string;
  published_site_limit: number;
  attachment_limit_bytes: number;
}

export interface MagicLinkResponse {
  success: boolean;
  message: string;
  dev_link?: string;
  dev_code?: string;
}

export interface NamespaceEntry {
  id: string;
  owner_user_id: string;
  created_at: number;
  metadata?: {
    type?: string;
    kind?: string;
    name?: string;
    provider?: string;
    [key: string]: unknown;
  } | null;
}

export function isWorkspaceNamespace(entry: NamespaceEntry): boolean {
  const metadataType = entry.metadata?.type;
  if (typeof metadataType === "string") {
    return metadataType === "workspace";
  }

  return entry.metadata?.kind === "workspace";
}

export interface UserStorageUsageResponse {
  used_bytes: number;
  blob_count: number;
  limit_bytes: number | null;
  warning_threshold: number;
  over_limit: boolean;
  scope: "attachments";
}

export interface PasskeyListItem {
  id: string;
  name: string;
  created_at: number;
  last_used_at: number | null;
}

export interface DeviceLimitDevice {
  id: string;
  name: string | null;
  last_seen_at: string;
}

export class AuthError extends Error {
  constructor(
    message: string,
    public statusCode: number,
    public details?: unknown,
    /** Present when statusCode === 403 and the error is a device limit error. */
    public devices?: DeviceLimitDevice[],
  ) {
    super(message);
    this.name = "AuthError";
  }
}

/**
 * Auth service for communicating with the sync server.
 */
export class AuthService {
  private serverUrl: string;

  constructor(serverUrl: string) {
    this.serverUrl = serverUrl.replace(/\/$/, ""); // Remove trailing slash
  }

  private async parseErrorBody(response: Response): Promise<unknown | null> {
    const contentType = response.headers.get("content-type") || "";
    if (!contentType.includes("application/json")) return null;
    try {
      return await response.json();
    } catch {
      return null;
    }
  }

  private errorMessageFromBody(data: unknown | null, fallback: string): string {
    if (!data || typeof data !== "object") return fallback;

    const error = (data as { error?: unknown }).error;
    if (typeof error === "string" && error.trim()) return error;

    const message = (data as { message?: unknown }).message;
    if (typeof message === "string" && message.trim()) return message;

    return fallback;
  }

  private async authErrorFromResponse(
    response: Response,
    fallback: string,
  ): Promise<AuthError> {
    const data = await this.parseErrorBody(response);
    const devices =
      data && typeof data === "object" && Array.isArray((data as any).devices)
        ? (data as any).devices
        : undefined;
    return new AuthError(
      this.errorMessageFromBody(data, fallback),
      response.status,
      data,
      devices,
    );
  }

  /** Build headers for an authenticated request. If authToken is provided, sets
   *  Authorization: Bearer. Otherwise relies on cookie (browser) or auto-injected
   *  header (Tauri proxyFetch). */
  private authHeaders(authToken?: string): Record<string, string> {
    if (authToken) {
      return { Authorization: `Bearer ${authToken}` };
    }
    return {};
  }

  /**
   * Request a magic link for the given email.
   */
  async requestMagicLink(email: string): Promise<MagicLinkResponse> {
    const response = await proxyFetch(`${this.serverUrl}/auth/magic-link`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ email }),
    });

    if (!response.ok) {
      const data = await this.parseErrorBody(response);
      const message = data && typeof data === "object" && "error" in data
        ? (data as any).error
        : `Server returned ${response.status}. Make sure this is a valid Diaryx sync server.`;
      throw new AuthError(message, response.status);
    }

    return response.json();
  }

  /**
   * Verify a magic link token and get session token.
   */
  async verifyMagicLink(
    token: string,
    deviceName?: string,
    replaceDeviceId?: string,
  ): Promise<VerifyResponse> {
    const url = new URL(`${this.serverUrl}/auth/verify`);
    url.searchParams.set("token", token);
    if (deviceName) {
      url.searchParams.set("device_name", deviceName);
    }
    if (replaceDeviceId) {
      url.searchParams.set("replace_device_id", replaceDeviceId);
    }

    const response = await proxyFetch(url.toString());

    if (!response.ok) {
      const data = await this.parseErrorBody(response);
      const errorObj = data && typeof data === "object" ? data as any : {};
      throw new AuthError(
        errorObj.error || "Failed to verify magic link",
        response.status,
        undefined,
        errorObj.devices,
      );
    }

    return response.json();
  }

  /**
   * Verify a 6-digit code and get session token.
   */
  async verifyCode(
    code: string,
    email: string,
    deviceName?: string,
    replaceDeviceId?: string,
  ): Promise<VerifyResponse> {
    const response = await proxyFetch(`${this.serverUrl}/auth/verify-code`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify({
        code,
        email,
        device_name: deviceName,
        replace_device_id: replaceDeviceId,
      }),
    });

    if (!response.ok) {
      const data = await this.parseErrorBody(response);
      const errorObj = data && typeof data === "object" ? data as any : {};
      throw new AuthError(
        errorObj.error || "Failed to verify code",
        response.status,
        undefined,
        errorObj.devices,
      );
    }

    return response.json();
  }

  /**
   * Get current user info.
   */
  async getMe(authToken?: string): Promise<MeResponse> {
    const response = await proxyFetch(`${this.serverUrl}/auth/me`, {
      headers: this.authHeaders(authToken),
    });

    if (!response.ok) {
      if (response.status === 401) {
        throw new AuthError("Session expired", 401);
      }
      throw new AuthError("Failed to get user info", response.status);
    }

    return response.json();
  }

  /**
   * Log out (delete session).
   */
  async logout(authToken?: string): Promise<void> {
    await proxyFetch(`${this.serverUrl}/auth/logout`, {
      method: "POST",
      headers: this.authHeaders(authToken),
    });
  }

  /**
   * Get user's devices.
   */
  async getDevices(authToken?: string): Promise<Device[]> {
    const response = await proxyFetch(`${this.serverUrl}/auth/devices`, {
      headers: this.authHeaders(authToken),
    });

    if (!response.ok) {
      throw await this.authErrorFromResponse(response, "Failed to get devices");
    }

    return response.json();
  }

  /**
   * Rename a device.
   */
  async renameDevice(authToken: string | undefined, deviceId: string, newName: string): Promise<void> {
    const response = await proxyFetch(`${this.serverUrl}/auth/devices/${deviceId}`, {
      method: "PATCH",
      headers: {
        ...this.authHeaders(authToken),
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ name: newName }),
    });

    if (!response.ok) {
      throw await this.authErrorFromResponse(response, "Failed to rename device");
    }
  }

  /**
   * Delete a device.
   */
  async deleteDevice(authToken: string | undefined, deviceId: string): Promise<void> {
    const response = await proxyFetch(`${this.serverUrl}/auth/devices/${deviceId}`, {
      method: "DELETE",
      headers: this.authHeaders(authToken),
    });

    if (!response.ok) {
      throw await this.authErrorFromResponse(response, "Failed to delete device");
    }
  }

  /**
   * Delete user account and all server data.
   */
  async deleteAccount(authToken?: string): Promise<void> {
    const response = await proxyFetch(`${this.serverUrl}/auth/account`, {
      method: "DELETE",
      headers: this.authHeaders(authToken),
    });

    if (!response.ok) {
      throw new AuthError("Failed to delete account", response.status);
    }
  }

  /**
   * Get server status.
   */
  async getStatus(): Promise<{
    status: string;
    version: string;
    active_connections: number;
  }> {
    const response = await proxyFetch(`${this.serverUrl}/api/status`);
    return response.json();
  }

  /**
   * List namespaces owned by the authenticated user.
   */
  async listNamespaces(authToken?: string): Promise<NamespaceEntry[]> {
    const response = await proxyFetch(`${this.serverUrl}/namespaces`, {
      headers: this.authHeaders(authToken),
    });

    if (!response.ok) {
      throw new AuthError("Failed to list namespaces", response.status);
    }

    return response.json();
  }

  // =========================================================================
  // Stripe Billing
  // =========================================================================

  /**
   * Create a Stripe Checkout Session for upgrading to Plus.
   * Returns the hosted checkout page URL.
   */
  async createCheckoutSession(authToken?: string): Promise<{ url: string }> {
    const response = await proxyFetch(`${this.serverUrl}/api/stripe/checkout`, {
      method: "POST",
      headers: this.authHeaders(authToken),
    });

    if (!response.ok) {
      throw new AuthError("Failed to create checkout session", response.status);
    }

    return response.json();
  }

  /**
   * Create a Stripe Customer Portal session for managing billing.
   * Returns the portal URL.
   */
  async createPortalSession(authToken?: string): Promise<{ url: string }> {
    const response = await proxyFetch(`${this.serverUrl}/api/stripe/portal`, {
      method: "POST",
      headers: this.authHeaders(authToken),
    });

    if (!response.ok) {
      throw new AuthError("Failed to create portal session", response.status);
    }

    return response.json();
  }

  // =========================================================================
  // Apple IAP
  // =========================================================================

  /**
   * Verify an Apple StoreKit 2 signed transaction with the server.
   * On success, the server upgrades the user to Plus tier.
   */
  async verifyAppleTransaction(
    authToken: string | undefined,
    signedTransaction: string,
    productId: string,
  ): Promise<{ success: boolean; tier: string }> {
    const response = await proxyFetch(
      `${this.serverUrl}/api/apple/verify-receipt`,
      {
        method: "POST",
        headers: {
          ...this.authHeaders(authToken),
          "Content-Type": "application/json",
        },
        body: JSON.stringify({
          signed_transaction: signedTransaction,
          product_id: productId,
        }),
      },
    );

    if (!response.ok) {
      const data = await response.json().catch(() => ({}));
      throw new AuthError(
        data.error || "Failed to verify Apple transaction",
        response.status,
      );
    }

    return response.json();
  }

  /**
   * Restore Apple IAP purchases by sending signed transactions to the server.
   */
  async restoreApplePurchases(
    authToken: string | undefined,
    signedTransactions: string[],
  ): Promise<{ success: boolean; restored_count: number; tier: string }> {
    const response = await proxyFetch(
      `${this.serverUrl}/api/apple/restore`,
      {
        method: "POST",
        headers: {
          ...this.authHeaders(authToken),
          "Content-Type": "application/json",
        },
        body: JSON.stringify({
          signed_transactions: signedTransactions,
        }),
      },
    );

    if (!response.ok) {
      const data = await response.json().catch(() => ({}));
      throw new AuthError(
        data.error || "Failed to restore Apple purchases",
        response.status,
      );
    }

    return response.json();
  }

  // =========================================================================
  // Passkeys (WebAuthn)
  // =========================================================================

  /**
   * Start passkey registration (requires auth).
   */
  async startPasskeyRegistration(
    authToken?: string,
  ): Promise<{ challenge_id: string; options: any }> {
    const response = await proxyFetch(
      `${this.serverUrl}/auth/passkeys/register/start`,
      {
        method: "POST",
        headers: {
          ...this.authHeaders(authToken),
          "Content-Type": "application/json",
        },
      },
    );
    if (!response.ok) {
      throw new AuthError("Failed to start passkey registration", response.status);
    }
    return response.json();
  }

  /**
   * Finish passkey registration.
   */
  async finishPasskeyRegistration(
    authToken: string | undefined,
    challengeId: string,
    name: string,
    credential: any,
  ): Promise<{ id: string }> {
    const response = await proxyFetch(
      `${this.serverUrl}/auth/passkeys/register/finish`,
      {
        method: "POST",
        headers: {
          ...this.authHeaders(authToken),
          "Content-Type": "application/json",
        },
        body: JSON.stringify({
          challenge_id: challengeId,
          name,
          credential,
        }),
      },
    );
    if (!response.ok) {
      const data = await response.json().catch(() => ({}));
      throw new AuthError(
        data.error || "Failed to finish passkey registration",
        response.status,
      );
    }
    return response.json();
  }

  /**
   * Start passkey authentication (public — no auth required).
   * If email is provided, scopes to that user's passkeys.
   * If omitted, uses discoverable credentials (browser picks).
   */
  async startPasskeyAuthentication(
    email?: string,
  ): Promise<{ challenge_id: string; options: any }> {
    const response = await proxyFetch(
      `${this.serverUrl}/auth/passkeys/authenticate/start`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(email ? { email } : {}),
      },
    );
    if (!response.ok) {
      const data = await response.json().catch(() => ({}));
      throw new AuthError(
        data.error || "Failed to start passkey authentication",
        response.status,
      );
    }
    return response.json();
  }

  /**
   * Finish passkey authentication (public — returns session).
   */
  async finishPasskeyAuthentication(
    challengeId: string,
    credential: any,
    deviceName?: string,
    replaceDeviceId?: string,
  ): Promise<VerifyResponse> {
    const response = await proxyFetch(
      `${this.serverUrl}/auth/passkeys/authenticate/finish`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          challenge_id: challengeId,
          credential,
          device_name: deviceName,
          replace_device_id: replaceDeviceId,
        }),
      },
    );
    if (!response.ok) {
      const data = await response.json().catch(() => ({}));
      throw new AuthError(
        data.error || "Failed to authenticate with passkey",
        response.status,
        undefined,
        data.devices,
      );
    }
    return response.json();
  }

  /**
   * List user's passkeys (requires auth).
   */
  async listPasskeys(
    authToken?: string,
  ): Promise<PasskeyListItem[]> {
    const response = await proxyFetch(`${this.serverUrl}/auth/passkeys`, {
      headers: this.authHeaders(authToken),
    });
    if (!response.ok) {
      throw new AuthError("Failed to list passkeys", response.status);
    }
    return response.json();
  }

  /**
   * Delete a passkey (requires auth).
   */
  async deletePasskey(authToken: string | undefined, id: string): Promise<void> {
    const response = await proxyFetch(`${this.serverUrl}/auth/passkeys/${id}`, {
      method: "DELETE",
      headers: this.authHeaders(authToken),
    });
    if (!response.ok) {
      throw new AuthError("Failed to delete passkey", response.status);
    }
  }
}

/**
 * Create an auth service instance.
 */
export function createAuthService(serverUrl: string): AuthService {
  return new AuthService(serverUrl);
}
