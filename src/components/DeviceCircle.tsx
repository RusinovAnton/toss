import { deviceEmoji, osFromModel } from "../lib/device";
import { DEVICE_DIAMETER, type Placement } from "../lib/radar";
import type { Device } from "../lib/tauri";
import { OsBadge } from "./OsBadge";

/** What the ring around a circle is saying right now. */
export type TransferPhase = "idle" | "active" | "done" | "error";

export interface Transfer {
  phase: TransferPhase;
  /** 0 to 1, across the whole session rather than one file. */
  progress: number;
  /** One line shown under the circle when something went wrong. */
  message?: string;
}

const ARC_RADIUS = DEVICE_DIAMETER / 2 + 2;
const ARC_BOX = (ARC_RADIUS + 3) * 2;
const CIRCUMFERENCE = 2 * Math.PI * ARC_RADIUS;

export function DeviceCircle({
  device,
  placement,
  transfer,
  hovered,
  trusted,
  paired,
  onClick,
  onMenu,
}: {
  device: Device;
  placement: Placement;
  transfer: Transfer;
  /** Files are being dragged over this circle. */
  hovered: boolean;
  /** Its transfers are accepted without asking. */
  trusted: boolean;
  /** Paired: accepted without asking, and the clipboard flows both ways. */
  paired: boolean;
  onClick: () => void;
  onMenu: (x: number, y: number) => void;
}) {
  const active = transfer.phase === "active";
  const percentage = Math.round(transfer.progress * 100);

  return (
    <button
      type="button"
      onClick={onClick}
      onContextMenu={(event) => {
        event.preventDefault();
        onMenu(placement.x, placement.y);
      }}
      className="device-appear absolute flex flex-col items-center outline-none"
      style={{
        left: placement.x,
        top: placement.y,
        transform: `translate(-50%, -50%) scale(${hovered ? 1.15 : 1})`,
        transition: "left 300ms ease, top 300ms ease, transform 200ms ease",
      }}
      title={`Click to send files to ${device.alias}, or drop them here`}
    >
      <span
        className="relative flex items-center justify-center rounded-full"
        style={{
          width: DEVICE_DIAMETER,
          height: DEVICE_DIAMETER,
          background: "var(--surface)",
          boxShadow: `0 2px 10px var(--shadow)`,
        }}
      >
        <svg
          className="pointer-events-none absolute"
          width={ARC_BOX}
          height={ARC_BOX}
          viewBox={`0 0 ${ARC_BOX} ${ARC_BOX}`}
        >
          {/* The resting ring, or the solid blue one while dragging. */}
          <circle
            cx={ARC_BOX / 2}
            cy={ARC_BOX / 2}
            r={ARC_RADIUS}
            fill="none"
            strokeWidth={hovered || paired ? 2 : trusted ? 1.5 : 1}
            className={
              transfer.phase === "done"
                ? "flash-ok"
                : transfer.phase === "error"
                  ? "flash-bad"
                  : undefined
            }
            style={{
              // A paired device wears a solid ring, so pairing is visible
              // without opening anything.
              // A trusted device wears a faint accent ring, a paired one a
              // solid one, so the two levels are visible at a glance.
              stroke:
                hovered || paired || trusted ? "var(--accent)" : "var(--ring)",
              opacity: !hovered && trusted && !paired ? 0.45 : paired && !hovered ? 0.8 : 1,
              transition: "stroke 150ms ease",
            }}
          />
          {/* The progress arc, filling clockwise from twelve o'clock. */}
          {active && (
            <circle
              cx={ARC_BOX / 2}
              cy={ARC_BOX / 2}
              r={ARC_RADIUS}
              fill="none"
              stroke="var(--accent)"
              strokeWidth={2}
              strokeLinecap="round"
              strokeDasharray={CIRCUMFERENCE}
              strokeDashoffset={CIRCUMFERENCE * (1 - transfer.progress)}
              transform={`rotate(-90 ${ARC_BOX / 2} ${ARC_BOX / 2})`}
              style={{ transition: "stroke-dashoffset 120ms linear" }}
            />
          )}
        </svg>
        <span className="text-2xl leading-none">{deviceEmoji(device.deviceType)}</span>
        <OsBadge os={osFromModel(device.deviceModel)} />
      </span>

      <span
        className="mt-1.5 max-w-[104px] truncate text-[11px] leading-tight"
        style={{ color: active ? "var(--fg)" : "var(--muted)" }}
      >
        {active ? `${percentage}%` : device.alias}
      </span>
      {transfer.phase === "error" && transfer.message && (
        <span className="text-[10px] leading-tight" style={{ color: "var(--bad)" }}>
          {transfer.message}
        </span>
      )}
    </button>
  );
}
