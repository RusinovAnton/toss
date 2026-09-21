# Toss

A file transfer app for your local network, shaped like a radar.

Toss speaks the [LocalSend](https://localsend.org) protocol, so it talks to the
official LocalSend app on any phone or computer already on your network. No
account, no cloud, no telemetry. Files go straight from one device to the other
over TLS.

The point of the project is the window.

<p align="center">
  <img src="docs/radar-dark.png" width="360" alt="The radar with seven nearby devices, one receiving at 42%">
</p>

## What it looks like

Your device sits in the middle with rings pulsing outward. Everyone the app can
see orbits around it.

| Nearby devices | An incoming request | Settings |
| --- | --- | --- |
| ![Light mode](docs/radar-light.png) | ![Incoming request](docs/incoming-dark.png) | ![Settings](docs/settings-dark.png) |

- **Send** by dragging files or a folder onto a circle. The circle grows, its
  ring turns blue, and letting go starts the transfer. No dialog, no confirm
  step. Click the middle instead if you would rather pick files.
- **Receive** by answering the card that slides up: Enter accepts, Escape
  declines. Files land in Downloads with their folder structure intact and
  never overwrite anything; a second `cat.png` becomes `cat (1).png`.
- **Close it** and Toss carries on from the menu bar, still receiving. Quit
  lives in that menu. It can start with your machine too, straight into the
  menu bar rather than on screen.
- **Watch** the ring around a circle fill as the transfer runs. Green when it
  lands, red with a reason when it does not.
- **Trust** a device the first time it sends you something, with the tick on
  the card. It stops asking from then on. Nothing else on the network gets that
  treatment; there is no accept-from-anyone switch.
- **Pair** a device to share clipboards as well. Copy on one machine, paste on
  the other, once the shared clipboard is on in settings. Pairing is the
  stronger step, so trusting a device does not hand over your clipboard.

Both pin that device's TLS certificate. Something else answering at the same
address is refused rather than trusted, so a device you know really is the one
you met.

## Install

Download the installer for your system from the
[releases page](../../releases), or build it yourself. Nothing is code-signed
yet, so both systems will want a word with you the first time.

### macOS

Open the `.dmg` and drag Toss to Applications.

Releases are signed ad-hoc rather than notarised by Apple, so the first launch
says **"Toss is damaged and can't be opened"**. Nothing is wrong with the
download: that is the quarantine flag your browser set, and macOS words it that
way for any app it cannot check with Apple. Two ways past it:

```bash
xattr -dr com.apple.quarantine /Applications/Toss.app
```

or try to open it, then go to **System Settings → Privacy & Security** and
press **Open Anyway**. Either one is needed once per install, so a new release
asks again. On macOS 14 and older, a right click then **Open** also works.

Only notarisation removes the warning for good, and that needs a paid Apple
Developer ID. The release workflow is ready for one: add the `APPLE_*` secrets
listed in `CLAUDE.md` and every build from then on is signed and notarised.

### Windows

Run the `.msi`. SmartScreen warns about an unknown publisher: choose **More
info**, then **Run anyway**.

**The first time Toss runs, Windows asks whether to allow it through the
firewall.** Say yes for private networks, otherwise no other device can reach
it and the radar stays empty.

### Build from source

You need [Rust](https://www.rust-lang.org/tools/install), Node 20 or newer and
[pnpm](https://pnpm.io).

```bash
pnpm install
pnpm tauri build
```

The bundles land in `src-tauri/target/release/bundle/`. A build only produces
the formats of the machine it runs on, so Windows installers come from a
Windows machine or from CI. Tagging a commit does that for you:

```bash
scripts/draft_release          # or --minor, or --major
```

Symlink it onto your PATH (`sudo ln -s "$PWD/scripts/draft_release"
/usr/local/bin/draft_release`) and it is `draft_release` from anywhere.

It pulls the default branch, works out the next version from the newest tag
and asks before pushing it. That builds macOS and Windows and leaves a draft
release with the installers attached.

## How it relates to LocalSend

LocalSend is the original, and the protocol is theirs. Toss is a separate
client that implements protocol v2.2 and stays wire-compatible on purpose, so
the two interoperate:

- Discovery over UDP multicast on `224.0.0.167:53317`, with an HTTP scan of the
  local `/24` when a router blocks multicast.
- Transfers over HTTPS on port `53317`, with self-signed certificates and
  devices identified by the SHA-256 fingerprint of their certificate.
- The same `prepare-upload`, `upload` and `cancel` routes, including the PIN
  and the checksum check.

Clipboard text travels as a LocalSend message, a single `text/*` file with the
text in `preview`, so the official app understands it too.

What Toss does not do, and LocalSend does: the download API, mobile builds, and
sending to several devices at once.

## Privacy

Toss reads your clipboard only when you send it, and writes it only when a
paired device sends you text. Nothing else touches it.

Everything stays on your network. Toss has no servers, collects nothing, and
never talks to the internet. The only things it writes are your identity, your
settings and the window position, in the app's own data directory.

## Licence

MIT. See [LICENSE](LICENSE).
