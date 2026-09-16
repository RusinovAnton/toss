import { deviceEmoji } from "../lib/device";
import { CENTER_DIAMETER } from "../lib/radar";
import type { IdentityInfo } from "../lib/tauri";

/**
 * This device, in the middle of the radar.
 *
 * Deliberately not clickable: everything you do is aimed at another device,
 * so the actions live on their circles rather than here.
 */
export function CenterCircle({
  identity,
  shaking,
}: {
  identity: IdentityInfo | null;
  /** Something was dropped on empty space. */
  shaking: boolean;
}) {
  return (
    <div
      className={`absolute top-1/2 left-1/2 flex flex-col items-center justify-center rounded-full ${
        shaking ? "shake" : ""
      }`}
      style={{
        width: CENTER_DIAMETER,
        height: CENTER_DIAMETER,
        transform: "translate(-50%, -50%)",
        background: "var(--surface)",
        boxShadow: `0 3px 18px var(--shadow)`,
        border: "1px solid var(--ring)",
      }}
    >
      <span className="text-3xl leading-none">{deviceEmoji(identity?.deviceType)}</span>
      <span
        className="mt-1 max-w-[84px] truncate text-[11px] leading-tight"
        style={{ color: "var(--muted)" }}
      >
        {identity?.alias ?? ""}
      </span>
    </div>
  );
}
