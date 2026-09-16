import { useEffect, useState } from "react";
import { formatBytes } from "./lib/format";
import {
  cancelSend,
  getIdentity,
  listDevices,
  onDevicesChanged,
  onIncomingRequest,
  onSessionFinished,
  onTransferProgress,
  rescan,
  respondToRequest,
  sendFiles,
  type Device,
  type IdentityInfo,
  type IncomingRequest,
  type TransferProgress,
} from "./lib/tauri";

// Placeholder shell. Phase 5 replaces all of this with the radar.
export default function App() {
  const [identity, setIdentity] = useState<IdentityInfo | null>(null);
  const [devices, setDevices] = useState<Device[]>([]);
  const [scanning, setScanning] = useState(false);
  const [request, setRequest] = useState<IncomingRequest | null>(null);
  const [progress, setProgress] = useState<TransferProgress | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [paths, setPaths] = useState("");
  const [sendingTo, setSendingTo] = useState<string | null>(null);
  const [session, setSession] = useState<string | null>(null);

  useEffect(() => {
    getIdentity().then(setIdentity).catch(console.error);
    listDevices().then(setDevices).catch(console.error);
    const unlisteners = [
      onDevicesChanged(setDevices),
      onIncomingRequest((incoming) => {
        setRequest(incoming);
        setStatus(null);
      }),
      onTransferProgress((update) => {
        setProgress(update);
        if (update.direction === "send") setSession(update.sessionId);
      }),
      onSessionFinished((finished) => {
        setRequest(null);
        setProgress(null);
        setStatus(
          finished.status === "completed"
            ? "Saved to Downloads"
            : finished.status === "cancelled"
              ? "Cancelled"
              : "Declined",
        );
      }),
    ];
    return () => {
      for (const unlisten of unlisteners) {
        unlisten.then((fn) => fn()).catch(console.error);
      }
    };
  }, []);

  async function handleRescan() {
    setScanning(true);
    try {
      setDevices(await rescan());
    } finally {
      setScanning(false);
    }
  }

  /// Sends whatever is typed in the path box. Retries once with a PIN if the
  /// peer asks for one, which is what the plan calls for.
  async function send(deviceId: string) {
    const list = paths
      .split("\n")
      .map((path) => path.trim())
      .filter(Boolean);
    if (list.length === 0) {
      setStatus("Type a path to send");
      return;
    }
    setSendingTo(deviceId);
    setStatus(null);
    try {
      await sendFiles(deviceId, list);
      setStatus("Sent");
    } catch (error) {
      const failure = error as { code?: string; message?: string };
      if (failure.code === "pin-required") {
        const pin = window.prompt("That device asks for a PIN");
        if (pin) {
          try {
            await sendFiles(deviceId, list, pin);
            setStatus("Sent");
            return;
          } catch (retry) {
            setStatus((retry as { message?: string }).message ?? "Failed");
            return;
          } finally {
            setSendingTo(null);
            setSession(null);
          }
        }
      }
      setStatus(failure.message ?? "Failed");
    } finally {
      setSendingTo(null);
      setSession(null);
    }
  }

  function answer(accept: boolean) {
    if (!request) return;
    const ids = accept ? request.files.map((file) => file.id) : [];
    respondToRequest(request.sessionId, ids).catch(console.error);
    setRequest(null);
  }

  const summary = request
    ? request.files.length === 1
      ? request.files[0].fileName
      : `${request.files.length} files · ${formatBytes(request.totalSize)}`
    : null;

  return (
    <main className="flex h-full w-full flex-col items-center gap-3 overflow-y-auto bg-[#F5F7FA] p-6 text-[#0B0F14] dark:bg-[#0B0F14] dark:text-[#F5F7FA]">
      <h1 className="text-2xl font-medium tracking-tight">Toss</h1>
      <p className="text-sm opacity-60">{identity?.alias ?? " "}</p>
      <button
        onClick={handleRescan}
        disabled={scanning}
        className="rounded-full border border-current/20 px-4 py-1 text-sm disabled:opacity-40"
      >
        {scanning ? "Scanning…" : "Scan"}
      </button>

      <ul className="w-full space-y-1 text-sm">
        {devices.map((device) => (
          <li
            key={device.fingerprint}
            className="flex items-center justify-between gap-2 rounded-lg bg-black/5 px-3 py-2 dark:bg-white/5"
          >
            <span className="truncate">{device.alias}</span>
            <span className="truncate opacity-50">
              {device.deviceModel ?? device.deviceType} · {device.ip}
            </span>
            <button
              onClick={() => send(device.fingerprint)}
              disabled={sendingTo !== null}
              className="shrink-0 rounded-full border border-current/20 px-3 py-0.5 text-xs disabled:opacity-40"
            >
              {sendingTo === device.fingerprint ? "Sending…" : "Send"}
            </button>
          </li>
        ))}
        {devices.length === 0 && (
          <li className="text-center opacity-40">No devices yet</li>
        )}
      </ul>

      <textarea
        value={paths}
        onChange={(event) => setPaths(event.target.value)}
        placeholder="One path per line (Phase 5 replaces this with drag and drop)"
        className="h-16 w-full rounded-lg bg-black/5 p-2 text-xs dark:bg-white/5"
      />

      {session && (
        <button
          onClick={() => cancelSend(session).catch(console.error)}
          className="rounded-full border border-current/20 px-3 py-0.5 text-xs"
        >
          Cancel send
        </button>
      )}

      {progress && (
        <p className="text-xs opacity-60">
          {progress.fileName} ·{" "}
          {Math.round((progress.bytesReceived / Math.max(progress.totalBytes, 1)) * 100)}%
        </p>
      )}
      {status && <p className="text-xs opacity-60">{status}</p>}

      {request && (
        <div className="fixed inset-x-4 bottom-4 rounded-2xl bg-white p-4 shadow-lg dark:bg-[#151B23]">
          <p className="text-sm font-medium">{request.sender.alias}</p>
          <p className="text-xs opacity-60">{summary}</p>
          <div className="mt-3 flex gap-2">
            <button
              onClick={() => answer(true)}
              className="flex-1 rounded-full bg-[#3B82F6] px-4 py-1.5 text-sm text-white"
            >
              Accept
            </button>
            <button
              onClick={() => answer(false)}
              className="flex-1 rounded-full border border-current/20 px-4 py-1.5 text-sm"
            >
              Deny
            </button>
          </div>
        </div>
      )}
    </main>
  );
}
