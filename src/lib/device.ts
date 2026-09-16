import type { DeviceType } from "./tauri";

/** The emoji shown inside a circle, from the protocol's `deviceType`. */
export function deviceEmoji(type: DeviceType | null | undefined): string {
  switch (type) {
    case "mobile":
      return "📱";
    case "web":
      return "🌐";
    case "headless":
      return "⚙️";
    case "server":
      return "🗄️";
    // The protocol says unknown values fall back to desktop.
    default:
      return "🖥️";
  }
}

export type OsKind = "apple" | "windows" | "linux" | "android" | "unknown";

/**
 * Guesses the operating system from the protocol's `deviceModel`.
 *
 * The strings the official app sends are recorded in CLAUDE.md: `macOS`,
 * `Windows`, `Linux`, a phone brand on Android, and the model name on iOS.
 */
export function osFromModel(model: string | null | undefined): OsKind {
  if (!model) return "unknown";
  const value = model.toLowerCase();

  // Android brands first: "Samsung Internet" is a browser on Android, and
  // several brand names would otherwise fall through to unknown.
  if (
    /android|pixel|samsung|xiaomi|redmi|oneplus|huawei|oppo|vivo|nothing|motorola|fire/.test(
      value,
    )
  ) {
    return "android";
  }
  if (/mac|iphone|ipad|ipod|ios|darwin|apple|safari/.test(value)) return "apple";
  if (/windows|edge|microsoft/.test(value)) return "windows";
  if (/linux|ubuntu|debian|fedora|arch|fuchsia/.test(value)) return "linux";
  return "unknown";
}
