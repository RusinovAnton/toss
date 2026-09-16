/**
 * Sample data for designing the radar in a plain browser.
 *
 * `pnpm dev` alone has no Tauri backend behind it, so every command would
 * throw and the window would stay empty. This fills it with stand-in devices
 * instead. It is dev-only and never reachable from the packaged app.
 */
import { isTauri } from "@tauri-apps/api/core";
import type { Device, IdentityInfo } from "./tauri";

export function isPreview(): boolean {
  return import.meta.env.DEV && !isTauri();
}

/** `?card`, `?saved` and `?settings` show those pieces in preview mode. */
export function previewFlag(name: string): boolean {
  return isPreview() && new URLSearchParams(window.location.search).has(name);
}

export const PREVIEW_REQUEST = {
  sessionId: "preview",
  sender: {
    alias: "Great Strawberry",
    fingerprint: "PEER-0",
    deviceModel: "Windows",
    ip: "192.168.1.10",
  },
  files: [
    { id: "a", fileName: "holiday.zip", size: 28_000_000, fileType: "application/zip" },
    { id: "b", fileName: "notes.md", size: 4_200, fileType: "text/markdown" },
    { id: "c", fileName: "cat.png", size: 14_000_000, fileType: "image/png" },
  ],
  totalSize: 42_004_200,
};

export const PREVIEW_IDENTITY: IdentityInfo = {
  alias: "Magic Lemon",
  fingerprint: "SELF",
  deviceModel: "macOS",
  deviceType: "desktop",
  port: 53317,
};

export const PREVIEW_DEVICES: Device[] = [
  ["Great Strawberry", "Windows", "desktop"],
  ["Secret Banana", "Pixel", "mobile"],
  ["Quiet Pineapple", "Linux", "desktop"],
  ["Brave Apricot", "iPhone", "mobile"],
  ["Sunny Olive", "Google Chrome", "web"],
  ["Tiny Fig", null, "headless"],
  ["Golden Melon", "Debian", "server"],
].map(([alias, deviceModel, deviceType], index) => ({
  fingerprint: `PEER-${index}`,
  alias: alias as string,
  deviceModel: deviceModel as string | null,
  deviceType: deviceType as Device["deviceType"],
  ip: `192.168.1.${10 + index}`,
  port: 53317,
  protocol: "https",
  download: false,
  lastSeen: Date.now(),
}));
