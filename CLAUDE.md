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
- `src-tauri/src/lib.rs` — Tauri builder, command registration
- `src-tauri/tauri.conf.json` — window (480x480, min 360), identifier `dev.toss.app`
- `src-tauri/capabilities/default.json` — permissions for the `main` window

## Rules

- All network I/O and filesystem access in Rust. Frontend only calls Tauri
  commands and listens to events.
- Never invent protocol fields. Copy names/semantics from the protocol repo.
- Never auto-overwrite files; collisions get ` (1)`, ` (2)` suffixes.
- Receiver must explicitly accept unless Quick Save is on (off by default).
- Ask before adding a dependency not listed in `PLAN.md`.
- Small conventional commits (`feat:`, `fix:`, `chore:`).

## Tauri commands

_None yet. Phase 1 adds `get_identity()`._

## Tauri events

_None yet._

## Protocol quirks

_None recorded yet. When the protocol doc is ambiguous, test against the
official LocalSend app and record the observed behaviour here._

## Phase status

- [x] Phase 0 — Scaffold
- [ ] Phase 1 — Identity and certificate
- [ ] Phase 2 — Discovery
- [ ] Phase 3 — Receive
- [ ] Phase 4 — Send
- [ ] Phase 5 — UI (radar)
- [ ] Phase 6 — Packaging
