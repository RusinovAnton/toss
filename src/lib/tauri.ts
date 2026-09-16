import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** Mirrors the protocol's `deviceType` enum. Unknown values arrive as `desktop`. */
export type DeviceType = "desktop" | "mobile" | "web" | "headless" | "server";
export type ProtocolType = "http" | "https";

export interface IdentityInfo {
  alias: string;
  fingerprint: string;
  deviceModel: string;
  deviceType: DeviceType;
  port: number;
}

export interface Device {
  /** The peer's certificate fingerprint. Stable id. */
  fingerprint: string;
  alias: string;
  deviceModel: string | null;
  deviceType: DeviceType;
  ip: string;
  port: number;
  protocol: ProtocolType;
  download: boolean;
  /** Unix milliseconds of the last confirmation. */
  lastSeen: number;
}

export function getIdentity(): Promise<IdentityInfo> {
  return invoke<IdentityInfo>("get_identity");
}

export function listDevices(): Promise<Device[]> {
  return invoke<Device[]>("list_devices");
}

/** Re-announces and scans the local /24. Resolves when the scan finishes. */
export function rescan(): Promise<Device[]> {
  return invoke<Device[]>("rescan");
}

export function onDevicesChanged(
  handler: (devices: Device[]) => void,
): Promise<UnlistenFn> {
  return listen<Device[]>("devices-changed", (event) => handler(event.payload));
}
