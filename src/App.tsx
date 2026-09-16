import { useEffect, useState } from "react";
import { formatBytes } from "./lib/format";
import {
  getIdentity,
  listDevices,
  onDevicesChanged,
  onIncomingRequest,
  onSessionFinished,
  onTransferProgress,
  rescan,
  respondToRequest,
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

  useEffect(() => {
    getIdentity().then(setIdentity).catch(console.error);
    listDevices().then(setDevices).catch(console.error);
    const unlisteners = [
      onDevicesChanged(setDevices),
      onIncomingRequest((incoming) => {
        setRequest(incoming);
        setStatus(null);
      }),
      onTransferProgress(setProgress),
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
            className="flex justify-between rounded-lg bg-black/5 px-3 py-2 dark:bg-white/5"
          >
            <span>{device.alias}</span>
            <span className="opacity-50">
              {device.deviceModel ?? device.deviceType} · {device.ip}
            </span>
          </li>
        ))}
        {devices.length === 0 && (
          <li className="text-center opacity-40">No devices yet</li>
        )}
      </ul>

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
