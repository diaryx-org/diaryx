export type TokenClaims = {
  s: string;
  a: string;
  t: string;
  e: number | null;
};

export async function validateSignedToken(
  signingKeyBase64: string,
  token: string,
): Promise<TokenClaims | null> {
  const [payloadB64, signatureB64] = token.split('.');
  if (!payloadB64 || !signatureB64) {
    return null;
  }

  const keyBytes = decodeBase64Flexible(signingKeyBase64);
  if (!keyBytes || keyBytes.byteLength !== 32) {
    return null;
  }

  const payloadBytes = decodeBase64Url(payloadB64);
  const signatureBytes = decodeBase64Url(signatureB64);
  if (!payloadBytes || !signatureBytes) {
    return null;
  }

  const key = await crypto.subtle.importKey(
    'raw',
    keyBytes,
    { name: 'HMAC', hash: 'SHA-256' },
    false,
    ['verify'],
  );

  const isValid = await crypto.subtle.verify('HMAC', key, signatureBytes, payloadBytes);
  if (!isValid) {
    return null;
  }

  const payloadText = new TextDecoder().decode(payloadBytes);
  const claims = JSON.parse(payloadText) as Partial<TokenClaims>;
  if (
    typeof claims.s !== 'string' ||
    typeof claims.a !== 'string' ||
    typeof claims.t !== 'string' ||
    !(typeof claims.e === 'number' || claims.e === null || claims.e === undefined)
  ) {
    return null;
  }

  return {
    s: claims.s,
    a: claims.a,
    t: claims.t,
    e: claims.e ?? null,
  };
}

function decodeBase64Url(input: string): Uint8Array | null {
  const normalized = input.replace(/-/g, '+').replace(/_/g, '/');
  const padded = normalized + '='.repeat((4 - (normalized.length % 4)) % 4);
  try {
    return Uint8Array.from(atob(padded), (c) => c.charCodeAt(0));
  } catch {
    return null;
  }
}

function decodeBase64Flexible(input: string): Uint8Array | null {
  const normalized = input.replace(/-/g, '+').replace(/_/g, '/');
  const padded = normalized + '='.repeat((4 - (normalized.length % 4)) % 4);
  try {
    return Uint8Array.from(atob(padded), (c) => c.charCodeAt(0));
  } catch {
    return null;
  }
}
