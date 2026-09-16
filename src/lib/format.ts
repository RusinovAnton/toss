const UNITS = ["B", "KB", "MB", "GB", "TB"];

/** Human-readable size, e.g. 42_000_000 -> "42 MB". Base 1000 like LocalSend. */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "0 B";
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < UNITS.length - 1) {
    value /= 1000;
    unit++;
  }
  const rounded = unit === 0 ? Math.round(value) : Number(value.toFixed(value < 10 ? 1 : 0));
  return `${rounded} ${UNITS[unit]}`;
}
