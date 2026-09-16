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
pnpm tauri build      # .dmg / .msi
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
- `src-tauri/src/lib.rs` — Tauri builder, `AppState`, command registration
- `src-tauri/src/identity.rs` — alias, rcgen cert, fingerprint, `identity.json` persistence
- `src-tauri/src/protocol.rs` — LocalSend v2 wire types (announce, register request/response)
- `src-tauri/src/discovery.rs` — multicast announce/listen, `/24` scan, device registry
- `src-tauri/tauri.conf.json` — window (480x480, min 360), identifier `dev.toss.app`
- `src-tauri/capabilities/default.json` — permissions for the `main` window

## Known limitations

- Discovery binds one interface: the one the 224.0.0.0/4 route points at. A machine on two LANs
  is only discovered on that one. Interface selection happens once at startup, so bringing a VPN
  up or changing network needs an app restart for multicast; the `/24` scan re-resolves per run.
- The subnet scan probes HTTPS only, matching the official app's default.
- IPv6 discovery is not implemented. It is a LocalSend extension on top of v2.2, not part of it.
- The official app holds TCP 53317 while it runs. Our Phase 3 server cannot bind that port on the
  same machine at the same time; test the two against each other from two machines, or stop one.
  UDP 53317 is shared fine, both use `SO_REUSEPORT`.

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

Frontend wrappers live in `src/lib/tauri.ts`. Always call through them, never `invoke` directly in components.

## Tauri events

| Event | Payload | When |
|---|---|---|
| `devices-changed` | `Device[]` | A peer appears, changes address/alias, or ages out. A pure last-seen refresh does not fire it |

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
- [ ] Phase 3 — Receive
- [ ] Phase 4 — Send
- [ ] Phase 5 — UI (radar)
- [ ] Phase 6 — Packaging
