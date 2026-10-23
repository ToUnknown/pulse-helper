# Pulse Helper

Optional, on-demand native capture for https://pulse.to-unknown.com. Browser sharing remains available. The website remembers your selected capture method locally.

Windows: run the installer (current user, no administrator needed). Requires a current NVIDIA driver / NVIDIA hardware encoder. macOS: Apple Silicon test build; unzip, move Pulse Helper.app to Applications, open once, and allow it in Privacy & Security if macOS blocks this unnotarized test build. Allow Screen & System Audio Recording when prompted. Requires macOS 27 or newer. Packaging checks every executable and bundled library against that minimum.

In Pulse, join a room, open Screen sharing, choose Native helper, and start. Allow the browser to open Pulse Helper. Capture starts from the website action without an additional helper confirmation dialog; operating-system permissions still apply. This test shares the main display at 1920×1080, up to 60 FPS, with system sound. System sound includes other applications, including call playback; headphones are recommended for this test. It does not change your display refresh rate. Other users can receive the stream in their browser without installing the helper.

The helper starts only for a sharing request, captures after the Pulse origin and launch token are verified, and exits when sharing stops or the controlling browser disconnects. No startup service is installed. Stop sharing in Pulse to end capture. The local bridge accepts only the two Pulse website origins and a random one-time launch token.

These builds are unsigned / ad-hoc signed and not notarized. Actual capture, encode and receive FPS depend on the machine and connection. This is an early test, not a guaranteed 60 FPS release. The test supports up to seven remote viewers plus the local preview. Each viewer currently has a separate encoder with an 18 Mbps maximum. There is no automatic updater yet.

## Releases

Download Windows and macOS installers from [GitHub Releases](https://github.com/ToUnknown/pulse-helper/releases).

## Source

Standalone native helper extracted from Pulse. The website keeps its browser bridge and links to GitHub Releases. This repository contains the native process and packaging tools. Changes here take effect after rebuilding and installing the helper.

## Build

Native source is in native/. Build with stable Rust and GStreamer 1.28 development libraries: cargo build --release --manifest-path native/Cargo.toml. Windows needs the MSVC toolchain and GStreamer MSVC x64. macOS needs Apple's command-line tools, GStreamer and libnice-gstreamer (`brew install gstreamer libnice-gstreamer`). Build and package on Apple Silicon with `MACOSX_DEPLOYMENT_TARGET=27.0 cargo build --release --locked --manifest-path native/Cargo.toml`, then `python3 scripts/package-mac.py`. Windows packaging uses scripts/stage-windows.ps1, scripts/prune-windows.py and NSIS scripts/installer.nsi. Runtime libraries are dynamically linked and can be replaced with compatible builds for debugging/modification.

## Third-party software

The distribution includes GStreamer and supporting open-source libraries. License notices are in licenses/ (macOS: Contents/Resources/licenses). The notices cover the upstream runtime distribution; this package ships a smaller subset. Upstream source and build recipes: https://gstreamer.freedesktop.org/src/ and https://gitlab.freedesktop.org/gstreamer/cerbero . macOS runtime recipes: https://github.com/Homebrew/homebrew-core . Pulse's native Cargo.lock pins Rust dependencies. Windows also includes Microsoft Visual C++ runtime components under Microsoft's redistribution terms.

## Packaging and releases

Windows: stage the runtime with `scripts/stage-windows.ps1`, prune unused DLLs with `python scripts/prune-windows.py`, and run `makensis installer.nsi` from `scripts/`. The installer is written to `release/Pulse-Helper-Windows.exe`. macOS packaging writes `release/Pulse-Helper-macOS-arm64.zip` and collects notices from the Homebrew libraries it bundles.

Upload both installers and a SHA-256 checksum file to a tagged GitHub release. Pulse links directly to this repository's Releases page; installers are not stored in the website source.

## Checks

Run `cargo test --locked --manifest-path native/Cargo.toml`. After building, run `python3 tests/bridge-smoke.py native/target/release/pulse-media` (on Windows, use Python and the `.exe` path). The smoke check verifies origin/token rejection, immediate readiness without a dialog, heartbeat and clean stop; it never requests screen or audio capture.
