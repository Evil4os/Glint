export function formatWidgetCountdown(value: string | null, now = Date.now()): string {
  if (!value) return "—";
  const reset = new Date(value).getTime();
  if (!Number.isFinite(reset)) return "—";
  const minutes = Math.max(0, Math.ceil((reset - now) / 60_000));
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  const remainder = minutes % 60;
  return remainder === 0 ? `${hours}h` : `${hours}h ${remainder}m`;
}

export function formatWidgetDate(value: string | null): string {
  if (!value) return "—";
  const reset = new Date(value);
  if (!Number.isFinite(reset.getTime())) return "—";
  return new Intl.DateTimeFormat("en-US", { month: "short", day: "numeric" }).format(reset);
}
