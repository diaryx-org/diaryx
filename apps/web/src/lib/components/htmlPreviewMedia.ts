import type { Api } from "$lib/backend/api";
import { resolveImageSrc } from "@/models/services/attachmentService";

type ImageResolver = (
  rawImagePath: string,
  entryPath: string,
  api: Api,
) => Promise<string | undefined>;

async function rewriteImgSrcAttributes(
  html: string,
  entryPath: string,
  api: Api,
  imageResolver: ImageResolver,
): Promise<string> {
  const imgSrcRegex = /<img\s[^>]*?\bsrc\s*=\s*(["'])((?:(?!\1).)+)\1/gi;
  let result = html;
  const replacements: { original: string; replacement: string }[] = [];

  let match: RegExpExecArray | null;
  while ((match = imgSrcRegex.exec(html)) !== null) {
    const [fullMatch, quote, rawSrc] = match;
    const resolvedSrc = await imageResolver(rawSrc.trim(), entryPath, api);
    if (resolvedSrc && resolvedSrc !== rawSrc.trim()) {
      replacements.push({
        original: fullMatch,
        replacement: fullMatch.replace(
          `${quote}${rawSrc}${quote}`,
          `${quote}${resolvedSrc}${quote}`,
        ),
      });
    }
  }

  for (const { original, replacement } of replacements) {
    result = result.replace(original, replacement);
  }

  return result;
}

async function rewriteSourceSrcsetAttributes(
  html: string,
  entryPath: string,
  api: Api,
  imageResolver: ImageResolver,
): Promise<string> {
  const sourceSrcsetRegex = /<source\s[^>]*?\bsrcset\s*=\s*(["'])((?:(?!\1).)+)\1/gi;
  let result = html;
  const replacements: { original: string; replacement: string }[] = [];

  let match: RegExpExecArray | null;
  while ((match = sourceSrcsetRegex.exec(html)) !== null) {
    const [fullMatch, quote, rawSrcset] = match;
    const candidates = rawSrcset
      .split(",")
      .map((candidate) => candidate.trim())
      .filter((candidate) => candidate.length > 0);

    if (candidates.length === 0) continue;

    const resolvedCandidates = await Promise.all(
      candidates.map(async (candidate) => {
        const candidateMatch = candidate.match(/^(\S+)(\s+.+)?$/);
        if (!candidateMatch) return candidate;

        const rawSrc = candidateMatch[1];
        const descriptor = candidateMatch[2] ?? "";
        const resolvedSrc = await imageResolver(rawSrc, entryPath, api);
        return resolvedSrc ? `${resolvedSrc}${descriptor}` : candidate;
      }),
    );

    const resolvedSrcset = resolvedCandidates.join(", ");
    if (resolvedSrcset !== rawSrcset.trim()) {
      replacements.push({
        original: fullMatch,
        replacement: fullMatch.replace(
          `${quote}${rawSrcset}${quote}`,
          `${quote}${resolvedSrcset}${quote}`,
        ),
      });
    }
  }

  for (const { original, replacement } of replacements) {
    result = result.replace(original, replacement);
  }

  return result;
}

export async function resolveHtmlPreviewMedia(
  html: string,
  entryPath: string,
  api: Api,
  imageResolver: ImageResolver = resolveImageSrc,
): Promise<string> {
  const withResolvedImgSrc = await rewriteImgSrcAttributes(
    html,
    entryPath,
    api,
    imageResolver,
  );
  return await rewriteSourceSrcsetAttributes(
    withResolvedImgSrc,
    entryPath,
    api,
    imageResolver,
  );
}
