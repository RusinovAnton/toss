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

export interface Settings {
  pin: string | null;
  quickSave: boolean;
}

export interface IncomingFile {
  id: string;
  fileName: string;
  size: number;
  fileType: string | null;
}

export interface IncomingRequest {
  sessionId: string;
  sender: {
    alias: string;
    fingerprint: string;
    deviceModel: string | null;
    ip: string;
  };
  files: IncomingFile[];
  totalSize: number;
}

export interface TransferProgress {
  sessionId: string;
  fileId: string;
  fileName: string;
  bytesReceived: number;
  totalBytes: number;
  direction: "receive" | "send";
}

export interface SessionFinished {
  sessionId: string;
  status: "completed" | "cancelled" | "declined";
  savedTo?: string;
  files?: string[];
}

/** Answers an incoming request. An empty list declines it. */
export function respondToRequest(
  sessionId: string,
  acceptedFileIds: string[],
): Promise<void> {
  return invoke("respond_to_request", { sessionId, acceptedFileIds });
}

export function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

export function setSettings(settings: Settings): Promise<Settings> {
  return invoke<Settings>("set_settings", { settings });
}

/** Where received files land. Not configurable in v1. */
export function getDownloadDir(): Promise<string> {
  return invoke<string>("download_dir");
}

export function onIncomingRequest(
  handler: (request: IncomingRequest) => void,
): Promise<UnlistenFn> {
  return listen<IncomingRequest>("incoming-request", (e) => handler(e.payload));
}

export function onTransferProgress(
  handler: (progress: TransferProgress) => void,
): Promise<UnlistenFn> {
  return listen<TransferProgress>("transfer-progress", (e) => handler(e.payload));
}

export function onSessionFinished(
  handler: (finished: SessionFinished) => void,
): Promise<UnlistenFn> {
  return listen<SessionFinished>("session-finished", (e) => handler(e.payload));
}

export function onDevicesChanged(
  handler: (devices: Device[]) => void,
): Promise<UnlistenFn> {
  return listen<Device[]>("devices-changed", (event) => handler(event.payload));
}
