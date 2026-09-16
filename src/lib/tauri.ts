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

/** Renames this device and tells peers straight away. */
export function setAlias(alias: string): Promise<IdentityInfo> {
  return invoke<IdentityInfo>("set_alias", { alias });
}

/** Reveals a received file in Finder or Explorer. */
export function showInFolder(path: string): Promise<void> {
  return invoke("show_in_folder", { path });
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
  /** Copy here, paste on a paired device. Off by default. */
  clipboardSync: boolean;
  /** Start with the machine, in the menu bar rather than on screen. */
  startAtLogin: boolean;
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
    /** What the sender claims. Only meaningful without encryption. */
    fingerprint: string;
    /** What the handshake proved. This is what pairing uses. */
    verifiedFingerprint: string | null;
    alias: string;
    deviceModel: string | null;
    ip: string;
  };
  files: IncomingFile[];
  totalSize: number;
  /** Set when this is clipboard text rather than files. */
  text: string | null;
}

export interface TransferProgress {
  sessionId: string;
  fileId: string;
  fileName: string;
  bytesReceived: number;
  totalBytes: number;
  /** Progress across the whole session, which is what the arc shows. */
  sessionDone: number;
  sessionTotal: number;
  direction: "receive" | "send";
  /** Fingerprint of the device on the other end, or null if it went away. */
  peer: string | null;
}

export interface SessionFinished {
  sessionId: string;
  status: "completed" | "cancelled" | "declined" | "error";
  direction?: "receive" | "send";
  peer?: string | null;
  reason?: string;
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

export interface TrustedDevice {
  /** SHA-256 of the device's certificate. Its identity. */
  fingerprint: string;
  alias: string;
  trustedAt: number;
}

export interface TextReceived {
  sessionId: string;
  peer: string | null;
  alias: string | null;
  text: string;
}

/** Devices this one is paired with. */
export function listTrusted(): Promise<TrustedDevice[]> {
  return invoke<TrustedDevice[]>("list_trusted");
}

/**
 * Pairs with a device. Its requests are then accepted without asking, and
 * clipboard text can flow both ways. The certificate is pinned, so a device
 * that later presents a different one is refused rather than trusted.
 */
export function trustDevice(deviceId: string): Promise<TrustedDevice> {
  return invoke<TrustedDevice>("trust_device", { deviceId });
}

export function untrustDevice(deviceId: string): Promise<boolean> {
  return invoke<boolean>("untrust_device", { deviceId });
}

/** Sends whatever is on the clipboard to a device, by hand. */
export function sendClipboard(deviceId: string): Promise<SendSummary> {
  return invoke<SendSummary>("send_clipboard", { deviceId });
}

/** Sends text to a paired device, landing on its clipboard. */
export function sendText(
  deviceId: string,
  text: string,
  pin?: string,
): Promise<SendSummary> {
  return invoke<SendSummary>("send_text", { deviceId, text, pin: pin ?? null });
}

export function onTextReceived(
  handler: (received: TextReceived) => void,
): Promise<UnlistenFn> {
  return listen<TextReceived>("text-received", (event) => handler(event.payload));
}

export interface SendSummary {
  sessionId: string;
  filesSent: number;
  bytesSent: number;
}

/** `code` is stable: declined, busy, pin-required, cancelled, connection-lost, ... */
export interface SendError {
  code: string;
  message: string;
}

/**
 * Sends files or folders to a discovered device. Resolves when every accepted
 * file has been uploaded. A `pin-required` error means the peer wants a PIN:
 * ask the user and call again with it.
 */
export function sendFiles(
  deviceId: string,
  paths: string[],
  pin?: string,
): Promise<SendSummary> {
  return invoke<SendSummary>("send_files", { deviceId, paths, pin: pin ?? null });
}

export function cancelSend(sessionId: string): Promise<void> {
  return invoke("cancel_send", { sessionId });
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
