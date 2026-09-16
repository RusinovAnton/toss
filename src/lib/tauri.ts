import { invoke } from "@tauri-apps/api/core";

/** Mirrors the protocol's `deviceType` enum. Unknown values fall back to desktop. */
export type DeviceType = "desktop" | "mobile" | "web" | "headless" | "server";

export interface IdentityInfo {
  alias: string;
  fingerprint: string;
  deviceModel: string;
  deviceType: DeviceType;
  port: number;
}

export function getIdentity(): Promise<IdentityInfo> {
  return invoke<IdentityInfo>("get_identity");
}
