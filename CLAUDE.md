# Toss

LocalSend-compatible desktop client (Tauri v2) with a radar UI. Speaks
LocalSend protocol v2 and must stay 100% wire-compatible with the official app.
Protocol reference: https://github.com/localsend/protocol

Full spec and phase plan: see `PLAN.md` (source of truth for scope).

## Stack

- Tauri v2 — Rust backend (`src-tauri/`), webview frontend
- React 19 + TypeScript + Tailwind v4 (`@tailwindcss/vite`, `@import "tailwindcss"` in `src/index.css`)
- Vite 8, Vitest for frontend tests
- pnpm for JS, cargo for Rust

## Run

```bash
pnpm install
pnpm tauri dev        # dev window, hot reload
pnpm tauri build      # bundles into src-tauri/target/release/bundle/
```

`pnpm tauri build` only produces the formats of the machine it runs on: `.dmg`
and `.app` on macOS, `.msi` and the NSIS installer on Windows. There is no
cross-compiling, so Windows bundles need a Windows machine or CI runner.

**The `.dmg` step needs `CI=true` here.** `bundle_dmg.sh` drives Finder over
AppleScript to lay the window out, which fails without automation permission
and takes the whole build down with it:

```bash
CI=true pnpm tauri build
```

`CI=true` skips the Finder pass. The image is identical apart from the window
layout when someone opens it. If a build dies at "Running bundle_dmg.sh", it
also leaves a half-built image mounted under `/Volumes/dmg.*` and a stray
`rw.*.dmg` in the bundle directory; `hdiutil detach` it before retrying, or a
second copy of the app keeps running and holds port 53317.

The icon comes from `assets/icon.svg`. After changing it, re-render the 1024px
PNG and regenerate the set:

```bash
pnpm tauri icon assets/icon.png
```

Rust toolchain via rustup (`~/.cargo/bin`). If `cargo` is not on PATH:
`source ~/.cargo/env`.

## Test

```bash
pnpm test                          # vitest run (src/**/*.test.ts)
cd src-tauri && cargo test         # Rust unit tests
```

## Layout

- `src/App.tsx` — root component (radar UI lives here later)
- `src/lib/` — pure TS helpers (formatting, geometry); unit-tested
- `src/lib/tauri.ts` — typed wrappers for every Tauri command / event
- `src/lib/radar.ts` — orbit geometry and drag hit testing; unit-tested
- `src/lib/device.ts` — device emoji and OS guessing; unit-tested
- `src/lib/preview.ts` — dev-only sample data, see Previewing the UI below
- `src/components/` — the radar: circles, pulses, cards, settings
- `src-tauri/src/lib.rs` — Tauri builder, `AppState`, command registration
- `src-tauri/src/identity.rs` — alias, rcgen cert, fingerprint, `identity.json` persistence
- `src-tauri/src/protocol.rs` — LocalSend v2 wire types (announce, register request/response)
- `src-tauri/src/discovery.rs` — multicast announce/listen, `/24` scan, device registry
- `src-tauri/src/server.rs` — axum HTTPS server, the five routes, TLS listener
- `src-tauri/src/send.rs` — folder walking, `prepare-upload` client, streaming uploads
- `src-tauri/src/session.rs` — one-at-a-time receive session, tokens, accept/decline
- `src-tauri/src/files.rs` — file name validation and collision suffixes
- `src-tauri/src/settings.rs` — `settings.json` (PIN, Quick Save)
- `src-tauri/src/window.rs` — remembered geometry and the square-window rule
- `src-tauri/src/trust.rs` — the paired devices, keyed by certificate fingerprint
- `src-tauri/src/tls.rs` — the certificate verifiers pairing rests on
- `src-tauri/tests/paired.rs` — pairing end to end over real TLS
- `assets/icon.svg` — the icon source; `src-tauri/icons/` is generated from it
- `docs/*.png` — README screenshots, captured from the dev preview
- `src-tauri/tests/receive.rs` — the receive routes end to end over plain HTTP
- `src-tauri/tests/send.rs` — the sender driven against our own receive router
- `src-tauri/tests/live_send.rs` — ignored by default; sends to a real peer on the network
- `src-tauri/tauri.conf.json` — window (480x480, min 360), identifier `dev.toss.app`
- `src-tauri/capabilities/default.json` — permissions for the `main` window

## Screenshots

The README's screenshots come from the dev preview, captured with headless
Chrome rather than a real window, because this repo is often worked on where
native screenshots are blocked:

```bash
pnpm dev
"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --headless=new \
  --window-size=480,480 --screenshot=docs/radar-dark.png \
  "http://localhost:1420/?theme=dark"
```

## Releases

`.github/workflows/ci.yml` runs both test suites on macOS and Windows.
`.github/workflows/release.yml` builds installers for macOS (Apple silicon and
Intel) and Windows on a `v*` tag, and leaves a **draft** release with them
attached, so nothing goes public without a look first:

```bash
git tag v0.1.0 && git push origin v0.1.0
```

Nothing is code-signed. Both systems warn on first launch; the release notes in
the workflow tell users what to click.

## Packaging

- Bundle identifier `dev.toss.app`, product name `Toss`, minimum macOS 10.15.
- The icon source is `assets/icon.svg`: a filled blue dot with two rings on a
  near-black squircle. It reads the same in one colour.
- Windows asks about the firewall on first launch. Without the private-network
  allowance no peer can reach the receive server and the radar stays empty.
  This is in the README because users hit it.

## Known limitations

- Discovery binds one interface: the one the 224.0.0.0/4 route points at. A machine on two LANs
  is only discovered on that one. Interface selection happens once at startup, so bringing a VPN
  up or changing network needs an app restart for multicast; the `/24` scan re-resolves per run.
- The subnet scan probes HTTPS only, matching the official app's default.
- IPv6 discovery is not implemented. It is a LocalSend extension on top of v2.2, not part of it.
- LocalSend's Dart app writes the checksum as `hash`, while the protocol doc and its own Rust
  core say `sha256`. We read `sha256` only, so a checksum from the Dart app is not verified.
  Verification is optional in the protocol, so this loses a check rather than breaking a transfer.
- The official app holds TCP 53317 while it runs. Our Phase 3 server cannot bind that port on the
  same machine at the same time; test the two against each other from two machines, or stop one.
  UDP 53317 is shared fine, both use `SO_REUSEPORT`.

## Receive behaviour worth knowing

- Nothing is written before the user accepts. The handler parks on a channel that
  `respond_to_request` feeds; Quick Save short-circuits it.
- Names are validated before a session is even created. `..`, absolute paths, Windows drive
  letters, NUL bytes and reserved device names (`NUL`, `COM1`, ...) are refused with `400`.
  Relative folders are kept, so folder sends preserve their structure.
- Collisions never overwrite: `cat.png`, `cat (1).png`, `cat (2).png`. The file is created with
  `create_new`, so two parallel uploads cannot both claim the same name.
- A failed, cancelled or checksum-mismatched transfer deletes its partial file.
- Settings are cached in memory. Editing `settings.json` by hand needs an app restart;
  `set_settings` applies at once.

## Send behaviour worth knowing

- Folders are walked and sent as one flat list with relative paths in `fileName`
  (`holiday/2024/cat.png`), which is how the receiver rebuilds the tree. Symlinks are skipped.
- Uploads are sequential. The protocol allows parallel ones, but one at a time gives honest
  progress and keeps the receiver writing one file.
- `sha256` is left out of the offer. It is nullable, and filling it means reading every file
  twice. The receive side still verifies it whenever a sender provides one.
- `prepare-upload` has no request deadline, only a 5s connect timeout: the peer's user may take
  a minute to answer. Progress events carry `direction: "send"`.
- Cancelling flips a flag the upload stream checks between chunks, then calls the peer's
  `/cancel`. The peer deletes its partial file.

To try a real send by hand:

```bash
TOSS_TARGET=192.168.1.5:53317 TOSS_SEND=/path/to/folder cargo test --test live_send -- --ignored --nocapture
```

## Pairing and the clipboard

Pairing a device does two things: its requests are accepted without asking, and
text can be passed between the two clipboards.

**What makes it safe.** A device is identified by the SHA-256 fingerprint of
its TLS certificate, taken from the handshake, never from the `fingerprint`
field in a request body. That field is trivially forged, which is why the
protocol itself says to ignore it in HTTPS mode.

- Our server asks every client for a certificate (`AnyClientCert`) and records
  its fingerprint per connection, reaching handlers as `ConnectInfo<Peer>`.
  Client certificates stay optional, so unpaired peers still work normally.
- Sending to a paired device uses a client pinned to its fingerprint
  (`PinnedServerCert`). A device answering at the same address with a different
  certificate is refused, and the failure is reported as `wrong-device` rather
  than a network error, via a flag the verifier raises.
- Over plain HTTP nothing is ever treated as paired: there is no certificate to
  go on. `PlainListener` always reports no fingerprint.

**Clipboard text on the wire** is exactly what LocalSend sends: a single file
whose `fileType` is `text/*` and whose `preview` holds the text. Toss spots
that shape and keeps it out of Downloads, emitting `text-received` instead. The
body is still read and dropped, so the sender sees an ordinary transfer.

**Shared clipboard** (settings, off by default) makes this seamless: a loop in
`clipboard.rs` watches the clipboard every 700ms and pushes changes to every
paired device that is currently on the radar. Copy on one machine, paste on the
other, no menu.

It is off by default on purpose. Pairing a device should not, by itself, start
a copy of everything you copy leaving the machine, passwords included. One
toggle turns it on.

Three details keep it sane. Received text is recorded as already-seen before it
is written, so two devices do not bounce a string forever. Only changes go out.
Anything over 64KB is skipped. The clipboard is read and written in Rust rather
than the frontend, which is both where the loop lives and why the webview needs
no clipboard permission at all.

In the UI: right-click a circle to pair, unpair or push the clipboard by hand.
A paired device wears a solid ring. With one device paired, Cmd/Ctrl+Shift+V
sends the clipboard without the menu.

## The radar

One window, no chrome. This device sits in the middle with blue rings pulsing outward; peers orbit
it, eight to a ring, first one at twelve o'clock and clockwise from there. The rings pause while
the window is in the background.

- **Sending**: drag files or folders onto a circle. It grows and its ring turns blue while you
  hover, and the drop starts the transfer with no confirmation. Clicking a circle opens a file
  picker aimed at that device; right-click for a folder picker or the clipboard. Dropping on empty
  space shakes the centre.
- **The centre circle does nothing on click.** Every action is aimed at another device, so the
  actions live on their circles. An earlier build opened a picker there and then asked which
  device to use, which read as a folder prompt out of nowhere.
- **Progress** is one arc per circle covering the whole session, not one file, with a percentage
  where the name usually is. Green flash on success, red flash plus a one-word reason otherwise.
- **Receiving**: a card slides up from the bottom. Enter accepts, Escape denies, and the receiver
  declines by itself after a minute. Quick Save skips the card. When it lands, the card offers
  "Show" to reveal the files in Finder.
- **Settings** live behind the gear: name, PIN, Quick Save. Nothing else in v1.
- The window is always square, at least 360px, and remembers where it was. Geometry is written at
  most every two seconds while dragging, so a crash still leaves a recent position behind.

### Previewing the UI

`pnpm dev` on its own has no backend, so the radar fills itself with sample devices. Handy for
design work in a normal browser:

```bash
pnpm dev     # then open http://localhost:1420/?card  (or ?saved, ?settings)
```

This only happens in a dev build outside Tauri, so the packaged app never shows it.

## Rules

- All network I/O and filesystem access in Rust. Frontend only calls Tauri
  commands and listens to events.
- Never invent protocol fields. Copy names/semantics from the protocol repo.
- Never auto-overwrite files; collisions get ` (1)`, ` (2)` suffixes.
- Receiver must explicitly accept unless Quick Save is on (off by default).
- Ask before adding a dependency not listed in `PLAN.md`.
- Small conventional commits (`feat:`, `fix:`, `chore:`).

## Tauri commands

| Command | Args | Returns | Notes |
|---|---|---|---|
| `get_identity` | — | `{ alias, fingerprint, deviceModel, deviceType, port }` | Loaded once in `setup` from `identity.json` in the app-data dir |
| `list_devices` | — | `Device[]` | Current peers. Snapshot; the event is the live feed |
| `rescan` | — | `Device[]` | Announce burst + `/24` scan. Resolves when the scan finishes (a few seconds) |
| `respond_to_request` | `sessionId`, `acceptedFileIds` | — | Answers an `incoming-request`. An empty list declines |
| `get_settings` | — | `{ pin, quickSave }` | |
| `set_settings` | `settings` | `Settings` | Persists and applies immediately; the server reads the live value |
| `download_dir` | — | `string` | Where received files land. Not configurable in v1 |
| `send_files` | `deviceId`, `paths`, `pin?` | `{ sessionId, filesSent, bytesSent }` | Resolves when every accepted file is uploaded. Folders are walked |
| `cancel_send` | `sessionId` | — | Aborts an in-flight send and tells the peer |
| `set_alias` | `alias` | `IdentityInfo` | Renames this device and re-announces at once |
| `show_in_folder` | `path` | — | Reveals a received file in Finder or Explorer |
| `list_trusted` | — | `TrustedDevice[]` | The paired devices |
| `trust_device` | `deviceId` | `TrustedDevice` | Pairs, pinning that device's certificate fingerprint |
| `untrust_device` | `deviceId` | `bool` | Unpairs |
| `send_text` | `deviceId`, `text`, `pin?` | `SendSummary` | Sends text, which lands on the peer's clipboard |
| `send_clipboard` | `deviceId` | `SendSummary` | Reads the clipboard in Rust and sends it |

`send_files` rejects with `{ code, message }`. Codes: `declined`, `busy`, `pin-required`,
`cancelled`, `connection-lost`, `no-files`, `too-many-requests`, `io-error`, `protocol-error`,
`unknown-device`. On `pin-required` the UI asks for a PIN and calls again with it.

Frontend wrappers live in `src/lib/tauri.ts`. Always call through them, never `invoke` directly in components.

## Tauri events

| Event | Payload | When |
|---|---|---|
| `devices-changed` | `Device[]` | A peer appears, changes address/alias, or ages out. A pure last-seen refresh does not fire it |
| `incoming-request` | `{ sessionId, sender, files, totalSize }` | A peer asked to send. Answer with `respond_to_request` within 60s or it is declined |
| `transfer-progress` | `{ sessionId, fileId, fileName, bytesReceived, totalBytes, direction }` | At most every 100ms per file, plus a final one. A single-chunk file produces exactly one |
| `session-finished` | `{ sessionId, status, savedTo?, files?, direction?, reason?, kind? }` | `status` is `completed`, `cancelled`, `declined` or `error`. Sends carry `direction: "send"`, messages carry `kind: "text"` |
| `text-received` | `{ sessionId, peer, alias, text }` | A message arrived. The UI writes it to the clipboard |

`Device` = `{ fingerprint, alias, deviceModel, deviceType, ip, port, protocol, download, lastSeen }`.
`fingerprint` is the stable id.

## Protocol quirks

Facts verified against the protocol repo and official app source (`packages/core`):

- **Fingerprint** = SHA-256 of the certificate **DER**, **uppercase hex, no colons** (64 chars).
  The official app compares fingerprints after `to_ascii_uppercase()`.
- **Certificate**: official app uses RSA-2048, `CN=LocalSend User`, no SANs, rcgen default
  validity (1975..4096, never expires). We use rcgen default ECDSA P-256, `CN=Toss`. Peers only
  check self-signature + time validity + fingerprint, so key type and CN do not matter.
- **Official app offers/expects mutual TLS**: its server calls `offer_client_auth() = true`
  (mandatory flag configurable). Our HTTPS client should present our own cert as client cert.
- **Current protocol version is 2.2** (app 1.18+). 2.1 added `pin` query + `401`/`429` + file
  `metadata`; 2.2 added `422` on sha256 mismatch on `/upload`. We announce `2.2`, so the receive
  path must verify `sha256` when provided.
- **`deviceModel` strings the official app sends**: `macOS`, `Windows`, `Linux`, `Fuchsia`;
  Android = brand in PascalCase (`Samsung`, `Google`, `Xiaomi`); iOS = `localizedModel`
  (`iPhone`, `iPad`); web = browser name (`Google Chrome`, `Firefox`, `Safari`, ...).
- Unknown `deviceType` values must be tolerated; the official app falls back to `desktop`.
- Default alias format is "Adjective Fruit" (e.g. "Nice Orange"). We mirror that.
- **Announces are answered over HTTP, not UDP.** The official app only ever *sends*
  multicast; the answer to an `announce: true` datagram is `POST /register` to the announcer.
  We do the same and fall back to a UDP datagram with `announce: false` when the HTTP call
  fails. A datagram with `announce: false` must never be answered.
- **Announce burst**: the official app repeats each announce after 100ms, 500ms and 2000ms,
  because a single datagram is easily lost. We copy those delays.
- The official app does *not* re-announce on a timer, so a peer that stays quiet would age out
  of our 60s window. We re-probe known peers every 20s to keep them alive.
- Register requests carry the client certificate; the official server offers client auth and can
  require it. Our HTTP client always presents ours.
- Peers use self-signed certificates, so certificate verification is off by design on our client.
  Trust comes from the fingerprint, which is what the official app pins too.
- **`prepare-upload` blocks** until the user answers. That is by design: the response body is the
  token map, so there is nothing to send before the decision. We time out at 60s and decline.
- **Status codes we return**: `200` accepted, `204` no files offered, `400` bad body or unsafe
  file name, `401` PIN required or wrong, `403` declined / bad token / wrong sender IP,
  `409` another session holds the receiver, `422` sha256 mismatch, `500` disk error.
- **Captured from LocalSend 1.18 on macOS** (2026-09-16), its announce datagram verbatim:

  ```json
  {"alias":"Great Strawberry","announce":true,"deviceModel":"macOS","deviceType":"desktop",
   "download":false,"fingerprint":"29E7E2...89EFA74","port":53317,"protocol":"https","version":"2.2"}
  ```

  Ours differs only in alias and fingerprint, so the wire shape is confirmed.

## Phase status

- [x] Phase 0 — Scaffold
- [x] Phase 1 — Identity and certificate
- [x] Phase 2 — Discovery
- [x] Phase 3 — Receive
- [x] Phase 4 — Send
- [x] Phase 5 — UI (radar)
- [x] Phase 6 — Packaging
