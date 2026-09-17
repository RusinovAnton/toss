import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open as openFilePicker } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { CenterCircle } from "./components/CenterCircle";
import { DeviceCircle, type Transfer } from "./components/DeviceCircle";
import { DeviceMenu } from "./components/DeviceMenu";
import { IncomingCard, SavedCard } from "./components/IncomingCard";
import { Pulses } from "./components/Pulses";
import { SettingsPanel } from "./components/SettingsPanel";
import {
  applyPreviewTheme,
  isPreview,
  previewFlag,
  PREVIEW_DEVICES,
  PREVIEW_IDENTITY,
  PREVIEW_REQUEST,
  PREVIEW_TEXT_REQUEST,
} from "./lib/preview";
import { hitTest, placeDevices } from "./lib/radar";
import {
  getIdentity,
  getSettings,
  listDevices,
  forgetDevice,
  listTrusted,
  pairDevice,
  sendClipboard,
  trustDevice,

  onDevicesChanged,
  onIncomingRequest,
  onSessionFinished,
  onTransferProgress,
  respondToRequest,
  sendFiles,
  setAlias as setAliasCommand,
  setSettings as setSettingsCommand,
  showInFolder,
  type Device,
  type IdentityInfo,
  type IncomingRequest,
  type Settings,
  type TrustedDevice,
} from "./lib/tauri";

const IDLE: Transfer = { phase: "idle", progress: 0 };
/** How long a green or red ring stays before the circle goes back to normal. */
const FLASH_MS = 900;
const ERROR_MS = 2600;
/** The receiver declines by itself after a minute; the card follows suit. */
const REQUEST_TIMEOUT_MS = 60_000;
const SAVED_NOTICE_MS = 5000;

const REASONS: Record<string, string> = {
  declined: "Declined",
  busy: "Busy",
  "connection-lost": "Connection lost",
  cancelled: "Cancelled",
  "pin-required": "PIN needed",
  "empty-clipboard": "Clipboard empty",
  "unknown-device": "Gone",
  "no-files": "Nothing to send",
};

export default function App() {
  const [identity, setIdentity] = useState<IdentityInfo | null>(null);
  const [devices, setDevices] = useState<Device[]>([]);
  const [settings, setSettings] = useState<Settings>({
    pin: null,
    clipboardSync: false,
    startAtLogin: false,
  });
  const [size, setSize] = useState(() => Math.min(window.innerWidth, window.innerHeight));
  const radarRef = useRef<HTMLElement | null>(null);
  const [transfers, setTransfers] = useState<Record<string, Transfer>>({});
  const [incoming, setIncoming] = useState<IncomingRequest | null>(() =>
    previewFlag("text") ? PREVIEW_TEXT_REQUEST : previewFlag("card") ? PREVIEW_REQUEST : null,
  );
  const [saved, setSaved] = useState<string[] | null>(() =>
    previewFlag("saved") ? ["/Users/me/Downloads/holiday.zip"] : null,
  );
  const [hovered, setHovered] = useState(-1);
  const [shaking, setShaking] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(() => previewFlag("settings"));
  const [trusted, setTrusted] = useState<TrustedDevice[]>([]);
  const [menu, setMenu] = useState<{ device: Device; x: number; y: number } | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  /// Ticked on the card: accept, and stop asking about this device.
  const [trustSender, setTrustSender] = useState(false);

  const placements = useMemo(() => placeDevices(devices.length, size), [devices.length, size]);
  // Drag events arrive outside React, so the hit test reads the latest
  // placements through a ref rather than a stale closure.
  const placementsRef = useRef(placements);
  placementsRef.current = placements;
  const devicesRef = useRef(devices);
  devicesRef.current = devices;
  const trustedRef = useRef(trusted);
  trustedRef.current = trusted;

  const isTrusted = (device: Device) =>
    trusted.some((entry) => entry.fingerprint === device.fingerprint);
  const isPaired = (device: Device) =>
    trusted.some((entry) => entry.fingerprint === device.fingerprint && entry.paired);

  const setTransfer = useCallback((peer: string, transfer: Transfer) => {
    setTransfers((current) => ({ ...current, [peer]: transfer }));
  }, []);

  const clearTransferLater = useCallback((peer: string, delay: number) => {
    window.setTimeout(() => {
      setTransfers((current) => {
        const { [peer]: _dropped, ...rest } = current;
        return rest;
      });
    }, delay);
  }, []);

  const send = useCallback(
    async (fingerprint: string, paths: string[], pin?: string) => {
      if (paths.length === 0) return;
      setTransfer(fingerprint, { phase: "active", progress: 0 });
      try {
        await sendFiles(fingerprint, paths, pin);
      } catch (error) {
        const failure = error as { code?: string; message?: string };
        if (failure.code === "pin-required" && pin === undefined) {
          const entered = window.prompt("That device asks for a PIN");
          if (entered) {
            await send(fingerprint, paths, entered);
            return;
          }
        }
        setTransfer(fingerprint, {
          phase: "error",
          progress: 0,
          message: REASONS[failure.code ?? ""] ?? "Failed",
        });
        clearTransferLater(fingerprint, ERROR_MS);
      }
    },
    [clearTransferLater, setTransfer],
  );

  // Live data and events.
  useEffect(() => {
    if (isPreview()) {
      applyPreviewTheme();
      setIdentity(PREVIEW_IDENTITY);
      setDevices(PREVIEW_DEVICES);
      setTrusted([
        {
          fingerprint: PREVIEW_DEVICES[0].fingerprint,
          alias: "Great Strawberry",
          trustedAt: 0,
          paired: true,
        },
        {
          fingerprint: PREVIEW_DEVICES[2].fingerprint,
          alias: "Quiet Pineapple",
          trustedAt: 0,
          paired: false,
        },
      ]);
      setTransfer(PREVIEW_DEVICES[1].fingerprint, { phase: "active", progress: 0.42 });
      return;
    }
    getIdentity().then(setIdentity).catch(console.error);
    listDevices().then(setDevices).catch(console.error);
    getSettings().then(setSettings).catch(console.error);
    listTrusted().then(setTrusted).catch(console.error);

    const unlisteners = [
      onDevicesChanged(setDevices),
      onIncomingRequest((request) => {
        setIncoming(request);
        setTrustSender(false);
      }),
      onTransferProgress((update) => {
        if (!update.peer) return;
        const total = Math.max(update.sessionTotal, 1);
        setTransfer(update.peer, {
          phase: "active",
          progress: Math.min(update.sessionDone / total, 1),
        });
      }),
      onSessionFinished((finished) => {
        setIncoming((current) =>
          current?.sessionId === finished.sessionId ? null : current,
        );
        if (finished.peer) {
          if (finished.status === "completed") {
            setTransfer(finished.peer, { phase: "done", progress: 1 });
            clearTransferLater(finished.peer, FLASH_MS);
          } else {
            setTransfer(finished.peer, {
              phase: "error",
              progress: 0,
              message: REASONS[finished.reason ?? finished.status] ?? "Failed",
            });
            clearTransferLater(finished.peer, ERROR_MS);
          }
        }
        // Clipboard text saves nothing, so the "saved to Downloads" card would
        // be a lie. A paired clipboard is meant to be silent: the text is
        // already on the clipboard by the time this arrives.
        if (
          finished.direction === "receive" &&
          finished.status === "completed" &&
          finished.kind !== "text"
        ) {
          setSaved(finished.files ?? []);
        }
      }),
    ];
    return () => {
      for (const unlisten of unlisteners) {
        unlisten.then((fn) => fn()).catch(console.error);
      }
    };
  }, [clearTransferLater, setTransfer]);

  // Measure the radar itself rather than the window: the observer reports the
  // real box as soon as it exists, which a resize listener does not.
  useEffect(() => {
    const element = radarRef.current;
    if (!element) return;
    const observer = new ResizeObserver(([entry]) => {
      const box = entry.contentRect;
      setSize(Math.min(box.width, box.height));
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  // Pausing the rings while the window is in the background keeps it cheap.
  useEffect(() => {
    if (isPreview()) return;
    const appWindow = getCurrentWindow();
    const unlisten = appWindow.onFocusChanged(({ payload: focused }) => {
      document.body.classList.toggle("unfocused", !focused);
    });
    appWindow
      .isFocused()
      .then((focused) => document.body.classList.toggle("unfocused", !focused))
      .catch(() => {});
    return () => {
      unlisten.then((fn) => fn()).catch(console.error);
    };
  }, []);

  // Files dragged in from Finder or Explorer.
  useEffect(() => {
    if (isPreview()) return;
    const unlisten = getCurrentWebview().onDragDropEvent((event) => {
      const payload = event.payload;
      if (payload.type === "leave") {
        setHovered(-1);
        return;
      }
      const ratio = window.devicePixelRatio || 1;
      const x = payload.position.x / ratio;
      const y = payload.position.y / ratio;
      const index = hitTest(placementsRef.current, x, y);

      if (payload.type === "over" || payload.type === "enter") {
        setHovered(index);
        return;
      }
      if (payload.type === "drop") {
        setHovered(-1);
        const device = devicesRef.current[index];
        // Kept because a drop that lands nowhere is the confusing case, and
        // this is the only way to see where it actually landed.
        console.info("drop", { x, y, index, alias: device?.alias, paths: payload.paths });
        if (index < 0 || !device) {
          // Dropping on empty space does nothing but say so.
          setShaking(true);
          window.setTimeout(() => setShaking(false), 400);
          return;
        }
        void send(device.fingerprint, payload.paths);
      }
    });
    return () => {
      unlisten.then((fn) => fn()).catch(console.error);
    };
  }, [send]);

  // Enter accepts, Escape denies or backs out.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (incoming) {
        if (event.key === "Enter") answer(true);
        if (event.key === "Escape") answer(false);
        return;
      }
      if (event.key === "Escape") {
        setSettingsOpen(false);
        setMenu(null);
      }
      // One paired device makes the clipboard a keystroke away.
      if (event.key.toLowerCase() === "v" && (event.metaKey || event.ctrlKey) && event.shiftKey) {
        const only = devicesRef.current.find((device) =>
          trustedRef.current.some((entry) => entry.fingerprint === device.fingerprint),
        );
        if (only) void pushClipboard(only);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  // The receiver declines on its own after a minute, so the card goes too.
  useEffect(() => {
    if (!incoming || isPreview()) return;
    const timer = window.setTimeout(() => setIncoming(null), REQUEST_TIMEOUT_MS);
    return () => window.clearTimeout(timer);
  }, [incoming]);

  useEffect(() => {
    if (!saved || isPreview()) return;
    const timer = window.setTimeout(() => setSaved(null), SAVED_NOTICE_MS);
    return () => window.clearTimeout(timer);
  }, [saved]);

  async function trust(device: Device) {
    setMenu(null);
    try {
      await trustDevice(device.fingerprint);
      setTrusted(await listTrusted());
      setNotice(`${device.alias} no longer asks`);
    } catch (error) {
      setNotice(String(error));
    }
  }

  async function setPaired(device: Device, paired: boolean) {
    setMenu(null);
    try {
      await pairDevice(device.fingerprint, paired);
      setTrusted(await listTrusted());
      setNotice(
        paired ? `Sharing the clipboard with ${device.alias}` : `Clipboard off for ${device.alias}`,
      );
    } catch (error) {
      setNotice(String(error));
    }
  }

  async function forget(device: Device) {
    setMenu(null);
    await forgetDevice(device.fingerprint).catch(console.error);
    setTrusted(await listTrusted().catch(() => []));
    setNotice(`Forgot ${device.alias}`);
  }

  async function pushClipboard(device: Device) {
    setMenu(null);
    setTransfer(device.fingerprint, { phase: "active", progress: 0 });
    try {
      await sendClipboard(device.fingerprint);
      setTransfer(device.fingerprint, { phase: "done", progress: 1 });
      clearTransferLater(device.fingerprint, FLASH_MS);
      setNotice(`Clipboard sent to ${device.alias}`);
    } catch (error) {
      const failure = error as { code?: string; message?: string };
      setTransfer(device.fingerprint, {
        phase: "error",
        progress: 0,
        message: REASONS[failure.code ?? ""] ?? "Failed",
      });
      clearTransferLater(device.fingerprint, ERROR_MS);
      if (failure.code === "empty-clipboard") setNotice("The clipboard is empty");
    }
  }

  function answer(accept: boolean) {
    if (!incoming) return;
    const ids = accept ? incoming.files.map((file) => file.id) : [];
    respondToRequest(incoming.sessionId, ids, accept && trustSender)
      .then(() => (accept && trustSender ? listTrusted().then(setTrusted) : undefined))
      .catch(console.error);
    setIncoming(null);
    setTrustSender(false);
  }

  /// Clicking a circle asks what to send it. Dragging onto it does the same
  /// thing without the dialog.
  async function pickFor(device: Device, directory = false) {
    setMenu(null);
    let picked;
    try {
      picked = await openFilePicker({ multiple: true, directory });
    } catch (error) {
      // A picker that refuses to open used to fail in complete silence, which
      // looks exactly like a click that did nothing.
      console.error("file picker failed", error);
      setNotice(`Could not open the picker: ${error}`);
      return;
    }
    if (!picked) return;
    const paths = Array.isArray(picked) ? picked : [picked];
    void send(device.fingerprint, paths);
  }

  return (
    <main
      ref={radarRef}
      className="relative h-full w-full overflow-hidden"
      style={{ background: "var(--bg)" }}
    >
      <Pulses size={size} />

      <CenterCircle identity={identity} shaking={shaking} />

      {devices.map((device, index) => (
        <DeviceCircle
          key={device.fingerprint}
          device={device}
          placement={placements[index]}
          transfer={transfers[device.fingerprint] ?? IDLE}
          hovered={hovered === index}
          trusted={isTrusted(device)}
          paired={isPaired(device)}
          onClick={() => void pickFor(device)}
          onMenu={(x, y) => setMenu({ device, x, y })}
        />
      ))}

      {menu && (
        <DeviceMenu
          device={menu.device}
          trusted={isTrusted(menu.device)}
          paired={isPaired(menu.device)}
          x={menu.x}
          y={menu.y}
          onSendFiles={() => void pickFor(menu.device)}
          onSendFolder={() => void pickFor(menu.device, true)}
          onTrust={() => void trust(menu.device)}
          onPair={() => void setPaired(menu.device, true)}
          onUnpair={() => void setPaired(menu.device, false)}
          onForget={() => void forget(menu.device)}
          onSendClipboard={() => void pushClipboard(menu.device)}
          onClose={() => setMenu(null)}
        />
      )}

      {notice && (
        <p
          className="absolute inset-x-0 bottom-3 text-center text-[11px]"
          style={{ color: "var(--muted)" }}
        >
          {notice}
        </p>
      )}

      {incoming ? (
        <IncomingCard
          request={incoming}
          trust={trustSender}
          onTrustChange={setTrustSender}
          onAccept={() => answer(true)}
          onDeny={() => answer(false)}
        />
      ) : saved ? (
        <SavedCard
          onShow={() => {
            if (saved[0]) showInFolder(saved[0]).catch(console.error);
            setSaved(null);
          }}
          onDismiss={() => setSaved(null)}
        />
      ) : (
        <SettingsPanel
          open={settingsOpen}
          alias={identity?.alias ?? ""}
          settings={settings}
          onToggle={() => setSettingsOpen((open) => !open)}
          onAlias={(alias) => {
            setAliasCommand(alias).then(setIdentity).catch(console.error);
          }}
          onSettings={(next) => {
            setSettings(next);
            setSettingsCommand(next).then(setSettings).catch(console.error);
          }}
        />
      )}
    </main>
  );
}
