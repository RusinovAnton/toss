/**
 * The badge on a device circle's top-left edge saying how far we trust it: a
 * green link when the clipboard is shared, a blue shield when transfers are
 * accepted without asking. Nothing at all for a device we do not know, which
 * is most of them.
 */
export function TrustBadge({ trusted, paired }: { trusted: boolean; paired: boolean }) {
  if (!trusted && !paired) return null;
  return (
    <span
      className="absolute top-0 left-0 flex h-[18px] w-[18px] -translate-x-1 -translate-y-1 items-center justify-center rounded-full"
      style={{
        background: "var(--surface)",
        border: "2px solid var(--bg)",
        color: paired ? "var(--linked)" : "var(--accent)",
      }}
      aria-hidden
    >
      {paired ? <LinkGlyph /> : <ShieldGlyph />}
    </span>
  );
}

function LinkGlyph() {
  return (
    <svg width={11} height={11} viewBox="0 0 24 24" fill="currentColor">
      <path d="M3.9 12c0-1.71 1.39-3.1 3.1-3.1h4V7H7a5 5 0 000 10h4v-1.9H7c-1.71 0-3.1-1.39-3.1-3.1zM8 13h8v-2H8v2zm9-6h-4v1.9h4c1.71 0 3.1 1.39 3.1 3.1s-1.39 3.1-3.1 3.1h-4V17h4a5 5 0 000-10z" />
    </svg>
  );
}

function ShieldGlyph() {
  return (
    <svg width={11} height={11} viewBox="0 0 24 24" fill="currentColor">
      <path d="M12 1L3 5v6c0 5.55 3.84 10.74 9 12 5.16-1.26 9-6.45 9-12V5l-9-4zm-2 16l-4-4 1.41-1.41L10 14.17l6.59-6.59L18 9l-8 8z" />
    </svg>
  );
}
