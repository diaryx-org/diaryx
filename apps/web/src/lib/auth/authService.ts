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
  ): Promise<Blob> {
    const response = await fetch(
      `${this.serverUrl}/api/workspaces/${encodeURIComponent(workspaceId)}/snapshot`,
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
  ): Promise<{ files_imported: number }> {
    const response = await fetch(
      `${this.serverUrl}/api/workspaces/${encodeURIComponent(workspaceId)}/snapshot?mode=${mode}`,
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
}

/**
 * Create an auth service instance.
 */
export function createAuthService(serverUrl: string): AuthService {
  return new AuthService(serverUrl);
}
