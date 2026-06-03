import type { BinaryRef } from "$lib/backend/generated";

interface AttachmentLookup {
  hash: string;
  mimeType: string;
  sizeBytes: number;
  workspaceId: string;
}

const attachmentIndex = new Map<string, AttachmentLookup>();

function metadataIndexKey(entryPath: string, attachmentPath: string): string {
  return `${entryPath}::${attachmentPath}`;
}

function stripMetadataIndexForEntry(entryPath: string): void {
  for (const key of attachmentIndex.keys()) {
    if (key.startsWith(`${entryPath}::`)) {
      attachmentIndex.delete(key);
    }
  }
}

export async function sha256Hex(bytes: Uint8Array): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", bytes as unknown as BufferSource);
  const hashBytes = new Uint8Array(digest);
  const hex = "0123456789abcdef";
  let value = "";
  for (let i = 0; i < hashBytes.length; i++) {
    value += hex[hashBytes[i] >> 4] + hex[hashBytes[i] & 0xf];
  }
  return value;
}

export function indexAttachmentRefs(
  entryPath: string,
  attachments: BinaryRef[],
  workspaceId: string,
): void {
  stripMetadataIndexForEntry(entryPath);
  for (const ref of attachments) {
    if (!ref.hash || ref.deleted) continue;
    attachmentIndex.set(metadataIndexKey(entryPath, ref.path), {
      hash: ref.hash,
      mimeType: ref.mime_type,
      sizeBytes: Number(ref.size ?? 0n),
      workspaceId,
    });
  }
}

export function getAttachmentMetadata(
  entryPath: string,
  attachmentPath: string,
): AttachmentLookup | null {
  return attachmentIndex.get(metadataIndexKey(entryPath, attachmentPath)) ?? null;
}

export type { AttachmentLookup };
