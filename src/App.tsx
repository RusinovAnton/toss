import { useEffect, useState } from "react";
import { getIdentity, type IdentityInfo } from "./lib/tauri";

export default function App() {
  const [identity, setIdentity] = useState<IdentityInfo | null>(null);

  useEffect(() => {
    getIdentity()
      .then(setIdentity)
      .catch(() => setIdentity(null));
  }, []);

  return (
    <main className="flex h-full w-full flex-col items-center justify-center gap-1 bg-[#F5F7FA] text-[#0B0F14] dark:bg-[#0B0F14] dark:text-[#F5F7FA]">
      <h1 className="text-2xl font-medium tracking-tight">Toss</h1>
      <p className="text-sm opacity-60">{identity?.alias ?? " "}</p>
    </main>
  );
}
