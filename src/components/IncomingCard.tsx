import { formatBytes } from "../lib/format";
import { deviceEmoji } from "../lib/device";
import type { IncomingRequest } from "../lib/tauri";

/**
 * The card that slides up from the bottom when a peer wants to send. Enter
 * accepts and Escape denies; the keys are wired up in App.
 */
export function IncomingCard({
  request,
  onAccept,
  onDeny,
}: {
  request: IncomingRequest;
  onAccept: () => void;
  onDeny: () => void;
}) {
  const summary =
    request.files.length === 1
      ? request.files[0].fileName
      : `${request.files.length} files · ${formatBytes(request.totalSize)}`;

  return (
    <Card>
      <div className="flex items-center gap-2">
        <span className="text-xl leading-none">{deviceEmoji("desktop")}</span>
        <div className="min-w-0 flex-1">
          <p className="truncate text-[13px] font-medium">{request.sender.alias}</p>
          <p className="truncate text-[11px]" style={{ color: "var(--muted)" }}>
            {summary}
          </p>
        </div>
      </div>
      <div className="mt-3 flex gap-2">
        <button
          type="button"
          onClick={onAccept}
          className="flex-1 rounded-full py-1.5 text-[12px] font-medium text-white"
          style={{ background: "var(--accent)" }}
        >
          Accept
        </button>
        <button
          type="button"
          onClick={onDeny}
          className="flex-1 rounded-full py-1.5 text-[12px]"
          style={{ border: "1px solid var(--ring)" }}
        >
          Deny
        </button>
      </div>
    </Card>
  );
}

/** Shown after a transfer lands, with a way into Finder. */
export function SavedCard({
  onShow,
  onDismiss,
}: {
  onShow: () => void;
  onDismiss: () => void;
}) {
  return (
    <Card>
      <div className="flex items-center gap-2">
        <p className="flex-1 text-[12px]">Saved to Downloads</p>
        <button
          type="button"
          onClick={onShow}
          className="rounded-full px-3 py-1 text-[11px]"
          style={{ border: "1px solid var(--ring)" }}
        >
          Show
        </button>
        <button
          type="button"
          onClick={onDismiss}
          className="text-[11px]"
          style={{ color: "var(--muted)" }}
          aria-label="Dismiss"
        >
          ✕
        </button>
      </div>
    </Card>
  );
}

function Card({ children }: { children: React.ReactNode }) {
  return (
    <div
      className="card-up absolute right-3 bottom-3 left-3 rounded-2xl p-3"
      style={{ background: "var(--surface)", boxShadow: `0 4px 24px var(--shadow)` }}
    >
      {children}
    </div>
  );
}
