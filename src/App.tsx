import { useEffect, useState } from "react";
import {
  getIdentity,
  listDevices,
  onDevicesChanged,
  rescan,
  type Device,
  type IdentityInfo,
} from "./lib/tauri";

// Placeholder shell. Phase 5 replaces all of this with the radar.
export default function App() {
  const [identity, setIdentity] = useState<IdentityInfo | null>(null);
  const [devices, setDevices] = useState<Device[]>([]);
  const [scanning, setScanning] = useState(false);

  useEffect(() => {
    getIdentity().then(setIdentity).catch(console.error);
    listDevices().then(setDevices).catch(console.error);
    const unlisten = onDevicesChanged(setDevices);
    return () => {
      unlisten.then((fn) => fn()).catch(console.error);
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
    </main>
  );
}
