/**
 * WebAuthn / Passkey browser utilities.
 *
 * Handles the translation between the server's JSON representation
 * and the browser's WebAuthn API (navigator.credentials).
 */

/** Check if WebAuthn is available in this browser. */
export function isWebAuthnAvailable(): boolean {
  return (
    typeof window !== "undefined" &&
    typeof window.PublicKeyCredential !== "undefined"
  );
}

/** Check if a platform authenticator (Touch ID, Face ID, Windows Hello) is available. */
export async function isPasskeySupported(): Promise<boolean> {
  if (!isWebAuthnAvailable()) return false;
  try {
    return await PublicKeyCredential.isUserVerifyingPlatformAuthenticatorAvailable();
  } catch {
    return false;
  }
}

// ── Base64url helpers ──

export function base64urlToBuffer(base64url: string): ArrayBuffer {
  const base64 = base64url.replace(/-/g, "+").replace(/_/g, "/");
  const pad = base64.length % 4;
  const padded = pad ? base64 + "=".repeat(4 - pad) : base64;
  const binary = atob(padded);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes.buffer;
}

export function bufferToBase64url(buffer: ArrayBuffer): string {
  const bytes = new Uint8Array(buffer);
  let binary = "";
  for (const b of bytes) {
    binary += String.fromCharCode(b);
  }
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

// ── Prepare options for browser API ──

/**
 * Transform server-sent creation options (JSON) into CredentialCreationOptions
 * for `navigator.credentials.create()`.
 */
export function prepareCreationOptions(
  serverOptions: any,
): CredentialCreationOptions {
  const publicKey = serverOptions.publicKey;

  // Decode challenge
  publicKey.challenge = base64urlToBuffer(publicKey.challenge);

  // Decode user.id
  if (publicKey.user?.id) {
    publicKey.user.id = base64urlToBuffer(publicKey.user.id);
  }

  // Decode excludeCredentials[].id
  if (publicKey.excludeCredentials) {
    publicKey.excludeCredentials = publicKey.excludeCredentials.map(
      (cred: any) => ({
        ...cred,
        id: base64urlToBuffer(cred.id),
      }),
    );
  }

  return { publicKey };
}

/**
 * Transform server-sent request options (JSON) into CredentialRequestOptions
 * for `navigator.credentials.get()`.
 */
export function prepareRequestOptions(
  serverOptions: any,
): CredentialRequestOptions {
  const publicKey = serverOptions.publicKey;

  // Decode challenge
  publicKey.challenge = base64urlToBuffer(publicKey.challenge);

  // Decode allowCredentials[].id
  if (publicKey.allowCredentials) {
    publicKey.allowCredentials = publicKey.allowCredentials.map(
      (cred: any) => ({
        ...cred,
        id: base64urlToBuffer(cred.id),
      }),
    );
  }

  return { publicKey };
}

// ── Serialize credential for server ──

/**
 * Serialize a registration credential (from navigator.credentials.create)
 * into JSON for the server.
 */
export function serializeRegistrationCredential(
  credential: PublicKeyCredential,
): any {
  const response = credential.response as AuthenticatorAttestationResponse;

  return {
    id: credential.id,
    rawId: bufferToBase64url(credential.rawId),
    type: credential.type,
    response: {
      attestationObject: bufferToBase64url(response.attestationObject),
      clientDataJSON: bufferToBase64url(response.clientDataJSON),
    },
  };
}

/**
 * Serialize an authentication credential (from navigator.credentials.get)
 * into JSON for the server.
 */
export function serializeAuthenticationCredential(
  credential: PublicKeyCredential,
): any {
  const response = credential.response as AuthenticatorAssertionResponse;

  return {
    id: credential.id,
    rawId: bufferToBase64url(credential.rawId),
    type: credential.type,
    response: {
      authenticatorData: bufferToBase64url(response.authenticatorData),
      clientDataJSON: bufferToBase64url(response.clientDataJSON),
      signature: bufferToBase64url(response.signature),
      userHandle: response.userHandle
        ? bufferToBase64url(response.userHandle)
        : null,
    },
  };
}
