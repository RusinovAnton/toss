# Project: Toss — a LocalSend-compatible desktop client with a radar UI

## Goal
Build a desktop app (macOS + Windows first, Linux nice-to-have) that speaks the LocalSend protocol v2, so it interoperates with the official LocalSend app on other devices. The point of the project is a better UI, not a new protocol. Stay 100% wire-compatible.

Protocol reference: https://github.com/localsend/protocol — read it fully before writing any network code. Do not invent message formats; copy field names and semantics exactly.

## Stack
- Tauri v2 (Rust backend, webview frontend)
- Frontend: React + TypeScript + Tailwind, Vite
- Rust crates: `tokio`, `axum` (HTTPS server), `reqwest` (client), `rcgen` (self-signed cert), `rustls`, `serde`/`serde_json`, `socket2` (multicast), `sha2`, `uuid`
- Package manager: pnpm

## Constraints
- Single binary, no external runtime.
- No cloud, no telemetry, no accounts. Everything on the local network.
- Receiver must explicitly accept every incoming request unless the user enables "Quick Save" in settings (off by default).
- Never auto-overwrite. On filename collision, append ` (1)`, ` (2)`, etc.
- All network I/O in Rust. Frontend never touches sockets or the filesystem directly; it only calls Tauri commands and listens to events.
- Keep a `CLAUDE.md` in the repo root updated with: how to run, how to test, protocol quirks discovered, and the list of Tauri commands and events.

## Phases
Work one phase at a time. At the end of each phase, run the tests and the manual check listed, then stop and report before starting the next.

### Phase 0 — Scaffold
- `pnpm create tauri-app toss` with React + TS template. Product name `Toss`, crate name `toss`.
- Add Tailwind. Empty window with app name.
- Set up `cargo test` and `pnpm test` (vitest). CI not required.
- Write initial `CLAUDE.md`.
- Manual check: `pnpm tauri dev` opens a window on macOS.

### Phase 1 — Identity and certificate
- On first launch, generate: device alias (random adjective + noun, editable later), a self-signed TLS cert + key via `rcgen`, and the SHA-256 fingerprint of the cert. Persist in the OS app-data directory as JSON.
- Expose Tauri command `get_identity()` returning alias, fingerprint, device model, device type (`desktop`), port (default 53317).
- Tests: identity is stable across restarts; fingerprint format matches what LocalSend expects (see protocol repo).

### Phase 2 — Discovery
- Join UDP multicast group `224.0.0.167`, port `53317`.
- On startup and every N seconds, send the announce message (`announce: true`) per protocol.
- On receiving an announce from another device, register it and reply per protocol (HTTP POST `/api/localsend/v2/register` to the announcer, falling back to a UDP response).
- Also implement the HTTP-scan fallback: if the user clicks "Scan", probe `/api/localsend/v2/register` on every host in the local /24 in parallel with a short timeout. Multicast is blocked on many routers.
- Maintain a device list with last-seen timestamp; drop devices not seen for 60s.
- Tauri: command `list_devices()`, command `rescan()`, event `devices-changed`.
- Manual check: official LocalSend on the Mac appears in our device list, and we appear in its list.

### Phase 3 — Receive
- Start an `axum` HTTPS server on port 53317 using our cert.
- Implement:
  - `POST /api/localsend/v2/register`
  - `POST /api/localsend/v2/prepare-upload` — store the pending session, emit event `incoming-request` with sender info and file list, wait for the user's decision (accept all / accept some / decline) via command `respond_to_request(session_id, accepted_file_ids)`. Return the protocol's token map on accept, `403` on decline.
  - `POST /api/localsend/v2/upload?sessionId=&fileId=&token=` — stream the body straight to disk in the configured download folder (default: OS Downloads). Validate token. Emit `transfer-progress` events (bytes received / total) at most every 100ms per file.
  - `POST /api/localsend/v2/cancel`
- Optional PIN: if set in settings, require the `pin` query param on `prepare-upload`, return `401` otherwise.
- Tests: `prepare-upload` without acceptance never writes to disk; wrong token → `403`; filename collision produces suffixed name; path traversal in filename (`../x`) is rejected.
- Manual check: send three files from official LocalSend to us; all land correctly and progress updates in the UI.

### Phase 4 — Send
- Command `send_files(device_id, paths[])`:
  1. Build file metadata (id, name, size, type, sha256 optional).
  2. `POST prepare-upload` to the target with our device info and file list.
  3. On `200`, upload each accepted file with its token using streaming request bodies. Emit `transfer-progress`.
  4. Handle `403` (declined), `409` (busy), `401` (PIN required → prompt user, retry once).
- Command `cancel_send(session_id)` calls the target's `/cancel`.
- Support folders: walk the tree, send files with relative paths in `fileName` as the protocol allows.
- Manual check: send a folder with nested subfolders to official LocalSend; structure preserved.

### Phase 5 — UI (radar)
Design goal: one window, no chrome, no lists, no sidebars. The whole app is a radar.

Layout
- Fixed-size window (default 480x480, resizable but always square; min 360). Plain background: `#0B0F14` in dark mode, `#F5F7FA` in light. Nothing else on screen except the circles described below and a tiny settings gear in the bottom-right corner at 40% opacity.
- Center: this device as a circle (diameter ~96px). Inside: device-type emoji (see table) and, underneath in small muted text, the alias.
- From the center circle, blue waves pulse outward continuously: 3 concentric rings, stroke `#3B82F6`, expanding from the circle's edge to the window edge over 2.4s, opacity fading 0.6 to 0, staggered 0.8s apart. Pure CSS keyframes, `transform: scale` + `opacity` only, so it stays cheap. Pause the animation when the window is not focused.
- Discovered devices: circles (diameter ~72px) placed on an orbit around the center at radius ~160px. Distribute evenly by angle; first device at 12 o'clock, then clockwise. When a device appears, it fades and scales in (200ms); when it disappears, fades out. Positions re-flow smoothly (300ms) when the count changes. Max 8 devices on the orbit; if more, add a second orbit at ~230px.
- Each device circle: device-type emoji in the middle, alias below in small text, and an OS badge — a small (18px) circle overlapping the bottom-right edge of the device circle, showing an OS glyph. OS glyphs are inline SVG, not emoji: Apple logo shape for macOS/iOS, Windows four-pane for Windows, Tux outline for Linux, Android bot head for Android. Fall back to a `?` if unknown. The badge has a thin border in the background color so it reads cleanly over the circle.

Device-type emoji (from the protocol's `deviceType` field)
- `desktop` → 🖥️
- `mobile` → 📱
- `web` → 🌐
- `headless` → ⚙️
- `server` → 🗄️
Derive OS from `deviceModel` (e.g. contains "Mac", "iPhone", "iPad" → Apple; "Windows" → Windows; "Linux" → Linux; "Android" / "Pixel" / "Samsung" → Android). Record the actual `deviceModel` strings the official app sends in `CLAUDE.md`.

Sending
- Drag files or folders from Finder/Explorer over any device circle. While hovering, that circle grows (scale 1.15) and its ring turns solid blue. Drop = send immediately, no confirmation dialog on the sender.
- During a transfer, the target circle's border becomes a circular progress arc (SVG stroke-dasharray) filling clockwise. Small percentage text replaces the alias while active. On completion the arc flashes green once and returns to normal. On decline or error the arc flashes red and a one-line tooltip explains ("Declined", "Busy", "Connection lost").
- Dropping onto empty space does nothing except a brief shake of the center circle.
- Clicking the center circle opens a native file picker as a fallback to drag-and-drop.

Receiving
- Incoming request: a compact card slides up from the bottom of the window (not a system modal): sender emoji + alias, one line summarizing "3 files · 42 MB" (or the single filename if one file), and two buttons: Accept and Deny. Enter = Accept, Esc = Deny. If Quick Save is enabled in settings, skip the card.
- Auto-deny after 60s with no answer.
- While receiving, the sender's circle shows the same progress arc as above.
- Files always go to the OS default Downloads folder (`dirs::download_dir()`), preserving folder structure for folder sends. On completion, the card shows "Saved to Downloads" with a "Show" button that reveals the files in Finder/Explorer, then disappears after 5s.

Settings (gear icon, small popover)
- Alias, PIN (optional), Quick Save toggle. Nothing else in v1. Download folder is not configurable in v1.

General
- Respect `prefers-color-scheme`.
- No text anywhere except aliases, the incoming card, the percentage, and settings labels.
- Window remembers position.

### Phase 6 — Packaging
- `pnpm tauri build` produces `.dmg` and `.msi`.
- Add an app icon: a filled circle with two thin concentric rings, blue on dark, monochrome-friendly. Bundle identifier `dev.toss.app`, product name `Toss`.
- On Windows, the first launch triggers a firewall prompt; document this in the README.
- README: what it is, screenshots, install, how it relates to LocalSend, license (MIT).

## Out of scope for v1
- Mobile builds
- Tray/menubar mode
- Send to multiple devices at once
- Text/clipboard sharing
- Favorites / trusted devices with pinned fingerprints (good v2 feature)

## Working style
- Small commits with conventional messages (`feat:`, `fix:`, `chore:`).
- When the protocol document is ambiguous, test against the official LocalSend app and record what it actually does in `CLAUDE.md` under "Protocol quirks".
- Ask before adding a dependency not listed above.
- Prefer boring, readable code over clever code.

## Definition of done
Two-way transfer of files and folders between this app and the official LocalSend app on the same wifi, with encryption, explicit accept, correct progress, and a UI someone would choose over the original.
