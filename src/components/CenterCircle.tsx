import { deviceEmoji } from "../lib/device";
import { CENTER_DIAMETER } from "../lib/radar";
import type { IdentityInfo } from "../lib/tauri";

/** This device, in the middle of the radar. Clicking it opens a file picker. */
export function CenterCircle({
  identity,
  shaking,
  armed,
  onClick,
}: {
  identity: IdentityInfo | null;
  /** Something was dropped on empty space. */
  shaking: boolean;
  /** Files are picked and waiting for a target. */
  armed: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`absolute top-1/2 left-1/2 flex flex-col items-center justify-center rounded-full outline-none ${
        shaking ? "shake" : ""
      }`}
      style={{
        width: CENTER_DIAMETER,
        height: CENTER_DIAMETER,
        transform: "translate(-50%, -50%)",
        background: "var(--surface)",
        boxShadow: `0 3px 18px var(--shadow)`,
        border: armed ? "2px solid var(--accent)" : "1px solid var(--ring)",
      }}
    >
      <span className="text-3xl leading-none">{deviceEmoji(identity?.deviceType)}</span>
      <span
        className="mt-1 max-w-[84px] truncate text-[11px] leading-tight"
        style={{ color: "var(--muted)" }}
      >
        {identity?.alias ?? ""}
      </span>
    </button>
  );
}
