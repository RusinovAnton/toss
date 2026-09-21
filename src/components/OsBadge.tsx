import type { OsKind } from "../lib/device";

/**
 * The small circle overlapping a device circle's bottom-right edge. Its border
 * is drawn in the page background so it reads cleanly over the circle.
 */
export function OsBadge({ os }: { os: OsKind }) {
  return (
    <span
      className="absolute right-0 bottom-0 flex h-[18px] w-[18px] translate-x-1 translate-y-1 items-center justify-center rounded-full"
      style={{
        background: "var(--surface)",
        border: "2px solid var(--bg)",
        color: "var(--fg)",
      }}
      aria-hidden
    >
      <Glyph os={os} />
    </span>
  );
}

function Glyph({ os }: { os: OsKind }) {
  const common = { width: 10, height: 10, viewBox: "0 0 24 24", fill: "currentColor" };
  switch (os) {
    case "apple":
      return (
        <svg {...common}>
          <path d="M16.4 1.4c0 1.1-.5 2.2-1.3 3-.8.9-2.2 1.5-3.3 1.4-.1-1.1.4-2.3 1.2-3.1.8-.8 2.3-1.4 3.4-1.3zM20.9 17.1c-.6 1.3-.8 1.9-1.5 3-1 1.6-2.4 3.6-4.1 3.6-1.6 0-1.9-1-4-1s-2.5 1-4.1 1c-1.7 0-3-1.8-4-3.4-2.9-4.4-3.2-9.6-1.4-12.4C3 6.3 4.9 5.4 6.7 5.4c1.8 0 3 1 4.5 1 1.5 0 2.4-1 4.5-1 1.6 0 3.3.9 4.5 2.4-4 2.2-3.3 7.9.7 9.3z" />
        </svg>
      );
    case "windows":
      // Four square panes, the Windows 11 mark, rather than the older
      // perspective flag: its slanted edges and uneven panes turn to mush at
      // 10px, and the two lower ones did not even meet in the middle.
      return (
        <svg {...common}>
          <path d="M3 3h8.5v8.5H3zm9.5 0H21v8.5h-8.5zM3 12.5h8.5V21H3zm9.5 0H21V21h-8.5z" />
        </svg>
      );
    case "linux":
      return (
        <svg {...common}>
          <path d="M12 2c-2.5 0-4 2-4 4.6 0 1.6-.3 2.6-1.3 4.2C5.4 12.8 4.5 14.6 4.5 17c0 2.9 3.2 5 7.5 5s7.5-2.1 7.5-5c0-2.4-.9-4.2-2.2-6.2-1-1.6-1.3-2.6-1.3-4.2C16 4 14.5 2 12 2zm-1.7 4.1c.5 0 .8.5.8 1.1s-.3 1.1-.8 1.1-.9-.5-.9-1.1.4-1.1.9-1.1zm3.4 0c.5 0 .9.5.9 1.1s-.4 1.1-.9 1.1-.8-.5-.8-1.1.3-1.1.8-1.1zM12 9.3c1 0 1.9.5 1.9 1s-.9 1.2-1.9 1.2-1.9-.7-1.9-1.2.9-1 1.9-1z" />
        </svg>
      );
    case "android":
      return (
        <svg {...common}>
          <path d="M6.8 8.2l-1.5-2.6a.4.4 0 01.7-.4l1.5 2.7a8.4 8.4 0 018.9 0l1.6-2.7a.4.4 0 01.7.4l-1.6 2.6A7 7 0 0121 14H3a7 7 0 013.8-5.8zM8.4 11a.8.8 0 100-1.6.8.8 0 000 1.6zm7.2 0a.8.8 0 100-1.6.8.8 0 000 1.6zM3 15.2h18v4.3a1.6 1.6 0 01-1.6 1.6H4.6A1.6 1.6 0 013 19.5v-4.3z" />
        </svg>
      );
    default:
      return (
        <span className="text-[9px] leading-none font-semibold" style={{ color: "var(--muted)" }}>
          ?
        </span>
      );
  }
}
