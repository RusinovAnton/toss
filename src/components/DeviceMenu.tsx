import { useLayoutEffect, useRef, useState } from "react";
import { MENU_WIDTH, menuPosition } from "../lib/menu";
import type { Device } from "../lib/tauri";

/**
 * The small menu behind a right click on a device circle. Pairing lives here
 * rather than on the circle itself, so the radar stays a radar.
 */
export function DeviceMenu({
  device,
  trusted,
  paired,
  x,
  y,
  size,
  onSendFiles,
  onSendFolder,
  onTrust,
  onPair,
  onUnpair,
  onForget,
  onSendClipboard,
  onClose,
}: {
  device: Device;
  /** Transfers from it are accepted without asking. */
  trusted: boolean;
  /** Trusted, and the clipboard flows both ways. */
  paired: boolean;
  x: number;
  y: number;
  /** The window's side, which the menu has to stay inside of. */
  size: number;
  onSendFiles: () => void;
  onSendFolder: () => void;
  onTrust: () => void;
  onPair: () => void;
  onUnpair: () => void;
  onForget: () => void;
  onSendClipboard: () => void;
  onClose: () => void;
}) {
  const box = useRef<HTMLDivElement>(null);
  // Which items are shown depends on trust, so the height is measured rather
  // than assumed. It is unknown for the first frame, which is what the
  // opacity below hides.
  const [height, setHeight] = useState(0);
  useLayoutEffect(() => {
    setHeight(box.current?.offsetHeight ?? 0);
  }, [device.fingerprint, trusted, paired]);

  const { left, top } = menuPosition({ x, y }, { width: MENU_WIDTH, height }, size);

  return (
    <>
      <div className="absolute inset-0" onClick={onClose} onContextMenu={onClose} />
      <div
        ref={box}
        className="absolute overflow-hidden rounded-xl py-1 text-[12px]"
        style={{
          left,
          top,
          width: MENU_WIDTH,
          opacity: height ? 1 : 0,
          background: "var(--surface)",
          boxShadow: "0 4px 24px var(--shadow)",
        }}
      >
        <p className="truncate px-3 pt-1.5 text-[11px]" style={{ color: "var(--muted)" }}>
          {device.alias}
        </p>
        {/* The address, because "which circle is this?" has no other answer. */}
        <p className="truncate px-3 pb-1.5 text-[10px]" style={{ color: "var(--muted)", opacity: 0.7 }}>
          {device.ip}:{device.port}
        </p>
        <Item onClick={onSendFiles}>Send files…</Item>
        <Item onClick={onSendFolder}>Send a folder…</Item>
        <Item onClick={onSendClipboard}>Send clipboard</Item>
        <div className="my-1 h-px" style={{ background: "var(--ring)" }} />
        {!trusted && <Item onClick={onTrust}>Trust, and stop asking</Item>}
        {paired ? (
          <Item onClick={onUnpair}>Stop sharing the clipboard</Item>
        ) : (
          <Item onClick={onPair}>Pair, and share the clipboard</Item>
        )}
        {trusted && <Item onClick={onForget}>Forget this device</Item>}
      </div>
    </>
  );
}

function Item({ onClick, children }: { onClick: () => void; children: React.ReactNode }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="block w-full px-3 py-1.5 text-left hover:bg-black/5 dark:hover:bg-white/10"
    >
      {children}
    </button>
  );
}
