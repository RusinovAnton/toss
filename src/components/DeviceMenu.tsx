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
  onSendFiles: () => void;
  onSendFolder: () => void;
  onTrust: () => void;
  onPair: () => void;
  onUnpair: () => void;
  onForget: () => void;
  onSendClipboard: () => void;
  onClose: () => void;
}) {
  return (
    <>
      <div className="absolute inset-0" onClick={onClose} onContextMenu={onClose} />
      <div
        className="absolute w-44 overflow-hidden rounded-xl py-1 text-[12px]"
        style={{
          left: Math.min(x, 9999),
          top: y,
          transform: "translate(-50%, 8px)",
          background: "var(--surface)",
          boxShadow: "0 4px 24px var(--shadow)",
        }}
      >
        <p className="truncate px-3 py-1.5 text-[11px]" style={{ color: "var(--muted)" }}>
          {device.alias}
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
