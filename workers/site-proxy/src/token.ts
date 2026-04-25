/**
 * Audience-access token shape, mirroring `diaryx_server::audience_token`.
 *
 * - `s`: namespace slug.
 * - `a`: audience name.
 * - `t`: token id (UUID, primarily for observability).
 * - `g`: gate kind the token was minted to satisfy.
 * - `pv`: password-gate version, present only when `g === 'unlock'`.
 * - `e`: optional unix-seconds expiry.
 *
 * Older tokens were issued before `g` existed; for backward compatibility we
 * treat an absent `g` as `link`.
 */
export type TokenClaims = {
  s: string;
  a: string;
  t: string;
  g: 'link' | 'unlock';
  pv: number | null;
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
    keyBytes.buffer as ArrayBuffer,
    { name: 'HMAC', hash: 'SHA-256' },
    false,
    ['verify'],
  );

  const isValid = await crypto.subtle.verify(
    'HMAC',
    key,
    signatureBytes.buffer as ArrayBuffer,
    payloadBytes.buffer as ArrayBuffer,
  );
  if (!isValid) {
    return null;
  }

  const payloadText = new TextDecoder().decode(payloadBytes);
  const claims = JSON.parse(payloadText) as Partial<TokenClaims> & {
    g?: unknown;
    pv?: unknown;
  };
  if (
    typeof claims.s !== 'string' ||
    typeof claims.a !== 'string' ||
    typeof claims.t !== 'string' ||
    !(typeof claims.e === 'number' || claims.e === null || claims.e === undefined)
  ) {
    return null;
  }

  // `g` defaults to "link" for back-compat with tokens minted before the
  // gate-stack rollout. "unlock" requires a numeric `pv`.
  const gate: 'link' | 'unlock' =
    claims.g === 'unlock' ? 'unlock' : 'link';
  let pv: number | null = null;
  if (gate === 'unlock') {
    if (typeof claims.pv !== 'number') {
      return null;
    }
    pv = claims.pv;
  }

  return {
    s: claims.s,
    a: claims.a,
    t: claims.t,
    g: gate,
    pv,
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
