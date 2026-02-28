/**
 * Audience color utilities.
 *
 * PALETTE — canonical list of Tailwind bg classes for audience dots.
 *   Listed as string literals so Tailwind's content scanner never purges them.
 *
 * hashAudienceColor(name) — deterministic hash → palette index.
 *   Call ONLY at audience-creation time; display should always read from
 *   the audienceColorStore.
 *
 * getAudienceColor(name, map) — PURE, side-effect-free reader.
 *   Returns the stored Tailwind class, or "bg-slate-500" as a safe fallback.
 *   MUST NOT mutate state or save anything.
 */

// Tailwind content scanner anchor — keep these literals present in source.
export const AUDIENCE_PALETTE = [
  "bg-indigo-500",
  "bg-teal-500",
  "bg-rose-500",
  "bg-amber-500",
  "bg-emerald-500",
  "bg-violet-500",
  "bg-cyan-500",
  "bg-orange-500",
] as const;

export type AudienceColor = (typeof AUDIENCE_PALETTE)[number];

/** Deterministic hash → AUDIENCE_PALETTE entry. For assignment only. */
export function hashAudienceColor(name: string): AudienceColor {
  let hash = 0;
  for (let i = 0; i < name.length; i++) hash = (hash * 31 + name.charCodeAt(i)) >>> 0;
  return AUDIENCE_PALETTE[hash % AUDIENCE_PALETTE.length];
}

/**
 * Pure reader — never mutates.
 * Returns the stored Tailwind class for `name`, or "bg-slate-500" if unknown.
 */
export function getAudienceColor(name: string, map: Record<string, string>): string {
  return map[name] ?? "bg-slate-500";
}
