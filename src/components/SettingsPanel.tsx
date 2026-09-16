import { useEffect, useState } from "react";
import type { Settings } from "../lib/tauri";

/** The gear in the corner and the small popover behind it. */
export function SettingsPanel({
  open,
  alias,
  settings,
  onToggle,
  onAlias,
  onSettings,
}: {
  open: boolean;
  alias: string;
  settings: Settings;
  onToggle: () => void;
  onAlias: (alias: string) => void;
  onSettings: (settings: Settings) => void;
}) {
  const [draftAlias, setDraftAlias] = useState(alias);
  const [draftPin, setDraftPin] = useState(settings.pin ?? "");

  // Keep the fields in step when the values change elsewhere.
  useEffect(() => setDraftAlias(alias), [alias]);
  useEffect(() => setDraftPin(settings.pin ?? ""), [settings.pin]);

  function commitAlias() {
    const value = draftAlias.trim();
    if (value && value !== alias) onAlias(value);
    else setDraftAlias(alias);
  }

  function commitPin() {
    const value = draftPin.trim();
    if (value !== (settings.pin ?? "")) {
      onSettings({ ...settings, pin: value === "" ? null : value });
    }
  }

  return (
    <>
      <button
        type="button"
        onClick={onToggle}
        aria-label="Settings"
        className="absolute right-3 bottom-3 text-base transition-opacity"
        style={{ opacity: open ? 1 : 0.4 }}
      >
        ⚙️
      </button>

      {open && (
        <div
          className="absolute right-3 bottom-11 w-52 rounded-2xl p-3 text-[12px]"
          style={{ background: "var(--surface)", boxShadow: `0 4px 24px var(--shadow)` }}
        >
          <label className="block">
            <span style={{ color: "var(--muted)" }}>Name</span>
            <input
              value={draftAlias}
              onChange={(event) => setDraftAlias(event.target.value)}
              onBlur={commitAlias}
              onKeyDown={(event) => event.key === "Enter" && commitAlias()}
              className="mt-1 w-full rounded-lg px-2 py-1 outline-none"
              style={{ background: "var(--bg)", border: "1px solid var(--ring)" }}
            />
          </label>

          <label className="mt-3 block">
            <span style={{ color: "var(--muted)" }}>PIN</span>
            <input
              value={draftPin}
              inputMode="numeric"
              placeholder="off"
              onChange={(event) => setDraftPin(event.target.value)}
              onBlur={commitPin}
              onKeyDown={(event) => event.key === "Enter" && commitPin()}
              className="mt-1 w-full rounded-lg px-2 py-1 outline-none"
              style={{ background: "var(--bg)", border: "1px solid var(--ring)" }}
            />
          </label>

          <label className="mt-2 flex items-center justify-between">
            <span style={{ color: "var(--muted)" }}>Shared clipboard</span>
            <input
              type="checkbox"
              checked={settings.clipboardSync}
              onChange={(event) =>
                onSettings({ ...settings, clipboardSync: event.target.checked })
              }
            />
          </label>
          <p className="mt-1 text-[10px] leading-snug" style={{ color: "var(--muted)" }}>
            Copy here, paste on a paired device.
          </p>

          <label className="mt-3 flex items-center justify-between">
            <span style={{ color: "var(--muted)" }}>Start at login</span>
            <input
              type="checkbox"
              checked={settings.startAtLogin}
              onChange={(event) =>
                onSettings({ ...settings, startAtLogin: event.target.checked })
              }
            />
          </label>
          <p className="mt-1 text-[10px] leading-snug" style={{ color: "var(--muted)" }}>
            Closing the window leaves Toss in the menu bar.
          </p>
        </div>
      )}
    </>
  );
}
