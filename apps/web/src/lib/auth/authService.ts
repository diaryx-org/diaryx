/**
 * Auth Service - Magic link authentication for Diaryx sync server.
 */

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

export interface UserHasDataResponse {
  has_data: boolean;
  file_count: number;
}

export interface UserStorageUsageResponse {
  used_bytes: number;
  blob_count: number;
  limit_bytes: number | null;
  warning_threshold: number;
  over_limit: boolean;
  scope: "attachments";
}

export interface InitAttachmentUploadRequest {
  entry_path: string;
  attachment_path: string;
  hash: string;
  size_bytes: number;
  mime_type: string;
  part_size?: number;
  total_parts?: number;
}

export interface InitAttachmentUploadResponse {
  upload_id: string | null;
  status: "uploading" | "already_exists";
  part_size: number;
  uploaded_parts: number[];
}

export interface CompleteAttachmentUploadRequest {
  entry_path: string;
  attachment_path: string;
  hash: string;
  size_bytes: number;
  mime_type: string;
}

export interface CompleteAttachmentUploadResponse {
  ok: boolean;
  blob_hash: string;
  r2_key: string;
  missing_parts: number[] | null;
}

export interface DownloadAttachmentResponse {
  bytes: Uint8Array;
  status: number;
  contentRange: string | null;
}

export interface StorageLimitExceededErrorResponse {
  error: "storage_limit_exceeded";
  message: string;
  used_bytes: number;
  limit_bytes: number;
  requested_bytes: number;
}

export interface PasskeyListItem {
  id: string;
  name: string;
  created_at: number;
  last_used_at: number | null;
}

export class AuthError extends Error {
  constructor(
    message: string,
    public statusCode: number,
    public details?: unknown,
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

  private formatQuotaMessage(
    payload: StorageLimitExceededErrorResponse,
  ): string {
    const usedMb = (payload.used_bytes / 1024 / 1024).toFixed(1);
    const limitMb = (payload.limit_bytes / 1024 / 1024).toFixed(1);
    return `${payload.message} (${usedMb} MB / ${limitMb} MB)`;
  }

  /**
   * Request a magic link for the given email.
   */
  async requestMagicLink(email: string): Promise<MagicLinkResponse> {
    const response = await fetch(`${this.serverUrl}/auth/magic-link`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ email }),
    });

    const data = await response.json();

    if (!response.ok) {
      throw new AuthError(
        data.error || "Failed to request magic link",
        response.status,
      );
    }

    return data;
  }

  /**
   * Verify a magic link token and get session token.
   */
  async verifyMagicLink(
    token: string,
    deviceName?: string,
  ): Promise<VerifyResponse> {
    const url = new URL(`${this.serverUrl}/auth/verify`);
    url.searchParams.set("token", token);
    if (deviceName) {
      url.searchParams.set("device_name", deviceName);
    }

    const response = await fetch(url.toString());
    const data = await response.json();

    if (!response.ok) {
      throw new AuthError(
        data.error || "Failed to verify magic link",
        response.status,
      );
    }

    return data;
  }

  /**
   * Verify a 6-digit code and get session token.
   */
  async verifyCode(
    code: string,
    email: string,
    deviceName?: string,
  ): Promise<VerifyResponse> {
    const response = await fetch(`${this.serverUrl}/auth/verify-code`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify({
        code,
        email,
        device_name: deviceName,
      }),
    });

    const data = await response.json();

    if (!response.ok) {
      throw new AuthError(
        data.error || "Failed to verify code",
        response.status,
      );
    }

    return data;
  }

  /**
   * Get current user info.
   */
  async getMe(authToken: string): Promise<MeResponse> {
    const response = await fetch(`${this.serverUrl}/auth/me`, {
      headers: {
        Authorization: `Bearer ${authToken}`,
      },
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
  async logout(authToken: string): Promise<void> {
    await fetch(`${this.serverUrl}/auth/logout`, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${authToken}`,
      },
    });
  }

  /**
   * Get user's devices.
   */
  async getDevices(authToken: string): Promise<Device[]> {
    const response = await fetch(`${this.serverUrl}/auth/devices`, {
      headers: {
        Authorization: `Bearer ${authToken}`,
      },
    });

    if (!response.ok) {
      throw new AuthError("Failed to get devices", response.status);
    }

    return response.json();
  }

  /**
   * Rename a device.
   */
  async renameDevice(authToken: string, deviceId: string, newName: string): Promise<void> {
    const response = await fetch(`${this.serverUrl}/auth/devices/${deviceId}`, {
      method: "PATCH",
      headers: {
        Authorization: `Bearer ${authToken}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ name: newName }),
    });

    if (!response.ok) {
      throw new AuthError("Failed to rename device", response.status);
    }
  }

  /**
   * Delete a device.
   */
  async deleteDevice(authToken: string, deviceId: string): Promise<void> {
    const response = await fetch(`${this.serverUrl}/auth/devices/${deviceId}`, {
      method: "DELETE",
      headers: {
        Authorization: `Bearer ${authToken}`,
      },
    });

    if (!response.ok) {
      throw new AuthError("Failed to delete device", response.status);
    }
  }

  /**
   * Delete user account and all server data.
   */
  async deleteAccount(authToken: string): Promise<void> {
    const response = await fetch(`${this.serverUrl}/auth/account`, {
      method: "DELETE",
      headers: {
        Authorization: `Bearer ${authToken}`,
      },
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
    const response = await fetch(`${this.serverUrl}/api/status`);
    return response.json();
  }

  /**
   * Check if user has synced data on the server.
   */
  async checkUserHasData(authToken: string): Promise<UserHasDataResponse> {
    const response = await fetch(`${this.serverUrl}/api/user/has-data`, {
      headers: {
        Authorization: `Bearer ${authToken}`,
      },
    });

    if (!response.ok) {
      throw new AuthError("Failed to check user data", response.status);
    }

    return response.json();
  }

  /**
   * Download a workspace snapshot zip from the server.
   */
  async downloadWorkspaceSnapshot(
    authToken: string,
    workspaceId: string,
    includeAttachments = true,
  ): Promise<Blob> {
    const url = new URL(
      `${this.serverUrl}/api/workspaces/${encodeURIComponent(workspaceId)}/snapshot`,
    );
    url.searchParams.set("include_attachments", String(includeAttachments));

    const response = await fetch(
      url.toString(),
      {
        headers: {
          Authorization: `Bearer ${authToken}`,
        },
      },
    );

    if (!response.ok) {
      throw new AuthError("Failed to download snapshot", response.status);
    }

    return response.blob();
  }

  /**
   * Upload a workspace snapshot zip to seed server CRDT state.
   */
  async uploadWorkspaceSnapshot(
    authToken: string,
    workspaceId: string,
    snapshot: Blob,
    mode: "replace" | "merge" = "replace",
    includeAttachments = true,
  ): Promise<{ files_imported: number }> {
    const url = new URL(
      `${this.serverUrl}/api/workspaces/${encodeURIComponent(workspaceId)}/snapshot`,
    );
    url.searchParams.set("mode", mode);
    url.searchParams.set("include_attachments", String(includeAttachments));

    const response = await fetch(
      url.toString(),
      {
        method: "POST",
        headers: {
          Authorization: `Bearer ${authToken}`,
          "Content-Type": "application/zip",
        },
        body: snapshot,
      },
    );

    if (!response.ok) {
      const data = await this.parseErrorBody(response);
      if (
        response.status === 413 &&
        data &&
        typeof data === "object" &&
        (data as any).error === "storage_limit_exceeded"
      ) {
        throw new AuthError(
          this.formatQuotaMessage(data as StorageLimitExceededErrorResponse),
          response.status,
          data,
        );
      }
      throw new AuthError("Failed to upload snapshot", response.status, data);
    }

    return response.json();
  }

  /**
   * Get attachment storage usage for the authenticated user.
   */
  async getUserStorageUsage(
    authToken: string,
  ): Promise<UserStorageUsageResponse> {
    const response = await fetch(`${this.serverUrl}/api/user/storage`, {
      cache: "no-store",
      headers: {
        Authorization: `Bearer ${authToken}`,
        "Cache-Control": "no-cache",
        Pragma: "no-cache",
      },
    });

    if (!response.ok) {
      throw new AuthError("Failed to fetch storage usage", response.status);
    }

    return response.json();
  }

  /**
   * Initialize a resumable attachment upload session.
   */
  async initAttachmentUpload(
    authToken: string,
    workspaceId: string,
    request: InitAttachmentUploadRequest,
  ): Promise<InitAttachmentUploadResponse> {
    const response = await fetch(
      `${this.serverUrl}/api/workspaces/${encodeURIComponent(workspaceId)}/attachments/uploads`,
      {
        method: "POST",
        headers: {
          Authorization: `Bearer ${authToken}`,
          "Content-Type": "application/json",
        },
        body: JSON.stringify(request),
      },
    );

    if (!response.ok) {
      const data = await this.parseErrorBody(response);
      if (
        response.status === 413 &&
        data &&
        typeof data === "object" &&
        (data as any).error === "storage_limit_exceeded"
      ) {
        throw new AuthError(
          this.formatQuotaMessage(data as StorageLimitExceededErrorResponse),
          response.status,
          data,
        );
      }
      throw new AuthError(
        "Failed to initialize attachment upload",
        response.status,
        data,
      );
    }

    return response.json();
  }

  /**
   * Upload one attachment multipart chunk.
   */
  async uploadAttachmentPart(
    authToken: string,
    workspaceId: string,
    uploadId: string,
    partNo: number,
    bytes: ArrayBuffer,
  ): Promise<{ ok: boolean; part_no: number }> {
    const response = await fetch(
      `${this.serverUrl}/api/workspaces/${encodeURIComponent(workspaceId)}/attachments/uploads/${encodeURIComponent(uploadId)}/parts/${partNo}`,
      {
        method: "PUT",
        headers: {
          Authorization: `Bearer ${authToken}`,
          "Content-Type": "application/octet-stream",
        },
        body: bytes,
      },
    );

    if (!response.ok) {
      throw new AuthError("Failed to upload attachment part", response.status);
    }

    return response.json();
  }

  /**
   * Complete a resumable attachment upload session.
   * Returns a conflict payload when missing parts are detected.
   */
  async completeAttachmentUpload(
    authToken: string,
    workspaceId: string,
    uploadId: string,
    request: CompleteAttachmentUploadRequest,
  ): Promise<CompleteAttachmentUploadResponse> {
    const response = await fetch(
      `${this.serverUrl}/api/workspaces/${encodeURIComponent(workspaceId)}/attachments/uploads/${encodeURIComponent(uploadId)}/complete`,
      {
        method: "POST",
        headers: {
          Authorization: `Bearer ${authToken}`,
          "Content-Type": "application/json",
        },
        body: JSON.stringify(request),
      },
    );

    if (response.status === 409) {
      return response.json();
    }

    if (!response.ok) {
      const data = await this.parseErrorBody(response);
      if (
        response.status === 413 &&
        data &&
        typeof data === "object" &&
        (data as any).error === "storage_limit_exceeded"
      ) {
        throw new AuthError(
          this.formatQuotaMessage(data as StorageLimitExceededErrorResponse),
          response.status,
          data,
        );
      }
      throw new AuthError(
        "Failed to complete attachment upload",
        response.status,
        data,
      );
    }

    return response.json();
  }

  /**
   * Download attachment bytes by hash for a workspace.
   */
  async downloadAttachment(
    authToken: string,
    workspaceId: string,
    hash: string,
    range?: { start: number; end?: number },
  ): Promise<DownloadAttachmentResponse> {
    const headers: Record<string, string> = {
      Authorization: `Bearer ${authToken}`,
    };
    if (range) {
      headers.Range = `bytes=${range.start}-${range.end ?? ""}`;
    }

    const response = await fetch(
      `${this.serverUrl}/api/workspaces/${encodeURIComponent(workspaceId)}/attachments/${encodeURIComponent(hash)}`,
      {
        headers,
      },
    );

    if (!response.ok) {
      throw new AuthError("Failed to download attachment", response.status);
    }

    const bytes = new Uint8Array(await response.arrayBuffer());
    return {
      bytes,
      status: response.status,
      contentRange: response.headers.get("Content-Range"),
    };
  }

  // =========================================================================
  // Workspace CRUD
  // =========================================================================

  /**
   * Create a new workspace.
   * Returns the created workspace object.
   * Throws 403 if workspace limit reached, 409 if name taken.
   */
  async createWorkspace(authToken: string, name: string): Promise<Workspace> {
    const response = await fetch(`${this.serverUrl}/api/workspaces`, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${authToken}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ name }),
    });

    if (!response.ok) {
      const data = await response.json().catch(() => ({}));
      if (response.status === 403) {
        throw new AuthError(data.error || "Workspace limit reached", 403);
      }
      if (response.status === 409) {
        throw new AuthError(data.error || "Workspace name already taken", 409);
      }
      throw new AuthError(data.error || "Failed to create workspace", response.status);
    }

    return response.json();
  }

  /**
   * Rename a workspace.
   */
  async renameWorkspace(authToken: string, workspaceId: string, newName: string): Promise<void> {
    const response = await fetch(`${this.serverUrl}/api/workspaces/${workspaceId}`, {
      method: "PATCH",
      headers: {
        Authorization: `Bearer ${authToken}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ name: newName }),
    });

    if (!response.ok) {
      const data = await response.json().catch(() => ({}));
      throw new AuthError(data.error || "Failed to rename workspace", response.status);
    }
  }

  /**
   * Delete a workspace.
   */
  async deleteWorkspace(authToken: string, workspaceId: string): Promise<void> {
    const response = await fetch(`${this.serverUrl}/api/workspaces/${workspaceId}`, {
      method: "DELETE",
      headers: {
        Authorization: `Bearer ${authToken}`,
      },
    });

    if (!response.ok) {
      const data = await response.json().catch(() => ({}));
      throw new AuthError(data.error || "Failed to delete workspace", response.status);
    }
  }

  // =========================================================================
  // Stripe Billing
  // =========================================================================

  /**
   * Create a Stripe Checkout Session for upgrading to Plus.
   * Returns the hosted checkout page URL.
   */
  async createCheckoutSession(authToken: string): Promise<{ url: string }> {
    const response = await fetch(`${this.serverUrl}/api/stripe/checkout`, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${authToken}`,
      },
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
  async createPortalSession(authToken: string): Promise<{ url: string }> {
    const response = await fetch(`${this.serverUrl}/api/stripe/portal`, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${authToken}`,
      },
    });

    if (!response.ok) {
      throw new AuthError("Failed to create portal session", response.status);
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
    authToken: string,
  ): Promise<{ challenge_id: string; options: any }> {
    const response = await fetch(
      `${this.serverUrl}/auth/passkeys/register/start`,
      {
        method: "POST",
        headers: {
          Authorization: `Bearer ${authToken}`,
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
    authToken: string,
    challengeId: string,
    name: string,
    credential: any,
  ): Promise<{ id: string }> {
    const response = await fetch(
      `${this.serverUrl}/auth/passkeys/register/finish`,
      {
        method: "POST",
        headers: {
          Authorization: `Bearer ${authToken}`,
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
    const response = await fetch(
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
  ): Promise<VerifyResponse> {
    const response = await fetch(
      `${this.serverUrl}/auth/passkeys/authenticate/finish`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          challenge_id: challengeId,
          credential,
          device_name: deviceName,
        }),
      },
    );
    if (!response.ok) {
      const data = await response.json().catch(() => ({}));
      throw new AuthError(
        data.error || "Failed to authenticate with passkey",
        response.status,
      );
    }
    return response.json();
  }

  /**
   * List user's passkeys (requires auth).
   */
  async listPasskeys(
    authToken: string,
  ): Promise<PasskeyListItem[]> {
    const response = await fetch(`${this.serverUrl}/auth/passkeys`, {
      headers: { Authorization: `Bearer ${authToken}` },
    });
    if (!response.ok) {
      throw new AuthError("Failed to list passkeys", response.status);
    }
    return response.json();
  }

  /**
   * Delete a passkey (requires auth).
   */
  async deletePasskey(authToken: string, id: string): Promise<void> {
    const response = await fetch(`${this.serverUrl}/auth/passkeys/${id}`, {
      method: "DELETE",
      headers: { Authorization: `Bearer ${authToken}` },
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
