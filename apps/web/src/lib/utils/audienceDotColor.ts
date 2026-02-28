// Tailwind classes listed here so the content scanner never purges them.
export const AUDIENCE_DOT_COLORS = [
  "bg-indigo-500", "bg-teal-500", "bg-rose-500",
  "bg-amber-500", "bg-emerald-500", "bg-violet-500",
] as const;

export function getAudienceDotColor(name: string): string {
  let hash = 0;
  for (let i = 0; i < name.length; i++) hash = (hash * 31 + name.charCodeAt(i)) >>> 0;
  return AUDIENCE_DOT_COLORS[hash % AUDIENCE_DOT_COLORS.length];
}
