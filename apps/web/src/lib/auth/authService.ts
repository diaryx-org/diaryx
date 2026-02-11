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
}

export interface MagicLinkResponse {
  success: boolean;
  message: string;
  dev_link?: string;
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

export class AuthError extends Error {
  constructor(
    message: string,
    public statusCode: number,
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
      throw new AuthError("Failed to upload snapshot", response.status);
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
      headers: {
        Authorization: `Bearer ${authToken}`,
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
      throw new AuthError("Failed to initialize attachment upload", response.status);
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
      throw new AuthError("Failed to complete attachment upload", response.status);
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
}

/**
 * Create an auth service instance.
 */
export function createAuthService(serverUrl: string): AuthService {
  return new AuthService(serverUrl);
}
