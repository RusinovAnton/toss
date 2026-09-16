import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open as openFilePicker } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { CenterCircle } from "./components/CenterCircle";
import { DeviceCircle, type Transfer } from "./components/DeviceCircle";
import { IncomingCard, SavedCard } from "./components/IncomingCard";
import { Pulses } from "./components/Pulses";
import { SettingsPanel } from "./components/SettingsPanel";
import {
  isPreview,
  previewFlag,
  PREVIEW_DEVICES,
  PREVIEW_IDENTITY,
  PREVIEW_REQUEST,
} from "./lib/preview";
import { hitTest, placeDevices } from "./lib/radar";
import {
  getIdentity,
  getSettings,
  listDevices,
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
  "unknown-device": "Gone",
  "no-files": "Nothing to send",
};

export default function App() {
  const [identity, setIdentity] = useState<IdentityInfo | null>(null);
  const [devices, setDevices] = useState<Device[]>([]);
  const [settings, setSettings] = useState<Settings>({ pin: null, quickSave: false });
  const [size, setSize] = useState(() => Math.min(window.innerWidth, window.innerHeight));
  const [transfers, setTransfers] = useState<Record<string, Transfer>>({});
  const [incoming, setIncoming] = useState<IncomingRequest | null>(() =>
    previewFlag("card") ? PREVIEW_REQUEST : null,
  );
  const [saved, setSaved] = useState<string[] | null>(() =>
    previewFlag("saved") ? ["/Users/me/Downloads/holiday.zip"] : null,
  );
  const [hovered, setHovered] = useState(-1);
  const [pending, setPending] = useState<string[] | null>(null);
  const [shaking, setShaking] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(() => previewFlag("settings"));

  const placements = useMemo(() => placeDevices(devices.length, size), [devices.length, size]);
  // Drag events arrive outside React, so the hit test reads the latest
  // placements through a ref rather than a stale closure.
  const placementsRef = useRef(placements);
  placementsRef.current = placements;
  const devicesRef = useRef(devices);
  devicesRef.current = devices;

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
      setIdentity(PREVIEW_IDENTITY);
      setDevices(PREVIEW_DEVICES);
      setTransfer(PREVIEW_DEVICES[1].fingerprint, { phase: "active", progress: 0.42 });
      return;
    }
    getIdentity().then(setIdentity).catch(console.error);
    listDevices().then(setDevices).catch(console.error);
    getSettings().then(setSettings).catch(console.error);

    const unlisteners = [
      onDevicesChanged(setDevices),
      onIncomingRequest(setIncoming),
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
        if (finished.direction === "receive" && finished.status === "completed") {
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

  // The window is square, so one number describes it.
  useEffect(() => {
    const onResize = () => setSize(Math.min(window.innerWidth, window.innerHeight));
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
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
        setPending(null);
        setSettingsOpen(false);
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

  function answer(accept: boolean) {
    if (!incoming) return;
    const ids = accept ? incoming.files.map((file) => file.id) : [];
    respondToRequest(incoming.sessionId, ids).catch(console.error);
    setIncoming(null);
  }

  async function pickFiles() {
    if (pending) {
      setPending(null);
      return;
    }
    const picked = await openFilePicker({ multiple: true });
    if (!picked) return;
    const paths = Array.isArray(picked) ? picked : [picked];
    if (devicesRef.current.length === 1) {
      void send(devicesRef.current[0].fingerprint, paths);
      return;
    }
    // With more than one device around, the next circle clicked gets them.
    setPending(paths);
  }

  function onDeviceClick(device: Device) {
    if (!pending) return;
    const paths = pending;
    setPending(null);
    void send(device.fingerprint, paths);
  }

  return (
    <main className="relative h-full w-full overflow-hidden" style={{ background: "var(--bg)" }}>
      <Pulses size={size} />

      <CenterCircle
        identity={identity}
        shaking={shaking}
        armed={pending !== null}
        onClick={() => void pickFiles()}
      />

      {devices.map((device, index) => (
        <DeviceCircle
          key={device.fingerprint}
          device={device}
          placement={placements[index]}
          transfer={transfers[device.fingerprint] ?? IDLE}
          hovered={hovered === index}
          armed={pending !== null}
          onClick={() => onDeviceClick(device)}
        />
      ))}

      {incoming ? (
        <IncomingCard
          request={incoming}
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
