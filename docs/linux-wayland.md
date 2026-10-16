# Linux Wayland sender and receiver

The user-requested Linux extension adds `vft-send-linux` and `vft-recv-linux`.
It reuses the existing v0 protocol, FEC, visual codec and sender/receiver sessions.
Windows adapters and the browser sender remain supported. X11 is not implemented.

## Build and runtime requirements

Rust 1.85 or newer, a C compiler, pkg-config, libclang (for PipeWire bindgen),
PipeWire/SPA development headers and libraries are needed to build the Linux receiver.
The sender dynamically loads Wayland and xkbcommon; it uses winit 0.30.13 and
softbuffer 0.4.8 with only the Wayland backend enabled. The receiver uses ashpd 0.11.0
and pipewire-rs 0.9.2. Linux desktop dependencies are target-specific and do not enter
the five `no_std + alloc` cores or Windows builds.

For example, Arch Linux build packages are `base-devel pkgconf clang pipewire`;
Debian/Ubuntu equivalents include `build-essential pkg-config libclang-dev
libpipewire-0.3-dev libspa-0.2-dev`. Runtime needs a running Wayland session, a session
D-Bus, PipeWire, `xdg-desktop-portal`, and a compositor-appropriate portal backend
providing ScreenCast (for example GNOME, KDE or a supported wlroots backend).
Install/configure the backend for your desktop; the applications do not change services.

```sh
cargo build --release -p vft-send-linux -p vft-recv-linux
./target/release/vft-send-linux input.bin --profile balanced --fps 15 --cell-size 4
./target/release/vft-recv-linux --auto --output recovered.bin --preview output/crop.bmp
```

Use `cargo metadata` to locate binaries if Cargo uses a custom target directory.
Diagnostic directories must already exist. Output, stats and preview paths must be
new and distinct; no file is silently overwritten. All filenames printed in terminal
controls are escaped for control characters.

## Presentation

The sender hashes an open regular file in 64 KiB chunks, then reads through
`ObjectSource::read_exact_at` with the core's single-generation cache. Length/mtime
changes are checked at source reads; keep input unchanged, including while its current
generation is cached. File contents are never loaded wholesale.

`--profile robust|balanced`, `--fps 1..60`, `--cell-size 2..32`, `--fullscreen`, and
`--frames N` are supported. Defaults are balanced, 15 FPS and 8 physical pixels per
cell (robust defaults to 16). ESC exits, Space pauses/resumes, F11 toggles fullscreen.
A resized/fullscreen window centers the complete grid with integer cells, reducing
cell size to fit. If fewer than two physical buffer pixels per cell fit, it clears
and pauses transport until enlarged. Known occlusion pauses advancement. Wayland does
not guarantee all occlusion events, topmost placement or window positioning; keep the
frame visible. No border/title bar is included in the rendered grid.

Surface buffers use winit's current physical inner size, including on scale changes;
no smooth image interpolation is applied by the application. Fractional scaling and
compositor/remote resampling still require physical-capture validation. A submitted
frame count and requested FPS do not establish observed screen refresh/unique FPS.

## Capture and recovery

The receiver opens the standard ScreenCast portal picker; select one monitor or
window containing a complete VFT frame. It restricts source types to the backend capabilities and prefers a hidden cursor,
then metadata-only cursor; if only embedded cursor is supported, it warns to keep the pointer
outside the frame. It does not persist capture permissions or a restore token. The portal session and authorized
PipeWire FD stay owned until capture stops; session close is attempted on success,
error, timeout and TUI cancellation. The timeout includes time spent in the picker.
Revocation/disconnect is an error rather than a successful transfer.

The stream negotiates one CPU-mapped BGRx/BGRA/RGBx/RGBA plane, at most 8192x8192
pixels. Row padding, chunk offsets/sizes and signed stride are validated before luma
conversion. Empty/corrupted chunks are skipped. Planar YUV, DMA-BUF-only producers,
HDR and transformed camera images are unsupported. There is no GPU import/conversion.
If the compositor cannot negotiate a supported CPU format, capture fails explicitly.
The capture handoff holds only the latest luma image, replacing stale frames. Recovery
remains memory-backed with the existing default **256 MiB file limit**; this is not
bounded-memory streaming recovery.

`--auto` locates a CRC-valid standard grid in the selected stream, searches at most
twice per second, and reacquires after 750 ms of continuous decode failure. Stream
size changes immediately invalidate the automatic location. Once a manifest selects
a session/object, subsequent searches stay with that identity and keep FEC progress.
`--search-rect X Y WIDTH HEIGHT` optionally narrows automatic discovery.

`--rect X Y WIDTH HEIGHT` instead supplies the exact visual frame rectangle.
All coordinates are **physical pixels in the selected PipeWire stream**, whose origin
is `(0, 0)`. Portal logical positions/global monitor coordinates are not used. A monitor
capture usually needs `--auto` to exclude surrounding windows/chrome. A window capture
may also include decorations or padding, so `--auto` remains recommended.
There is no `--sender-window`, global coordinate lookup, X11 or compositor-specific
screencopy fallback.

Other options: `--profile auto|robust|balanced`, `--fps 1..120` (default 30, processing
ceiling), `--timeout SECONDS` (default 3600), `--max-file-size BYTES`, `--stats FILE.csv`,
and `--preview FILE.bmp`. The preview is the first successfully decoded crop. Output
is written only after full-file SHA-256 verification, using ordered verified chunks
without an additional whole-file copy.

Stats record elapsed wall time including the picker, verified bytes and B/s (zero on
failure), processed captures, valid frames, invalid decode attempts, discovery scans,
location locks and requested processing FPS. Valid frames include duplicates. Capture
processing is paced and discovery is throttled; these counters are not video frame
loss, observed display FPS or symbol error rates. CPU, memory, codec bitrate and remote
settings are unmeasured. Failed runs retain diagnostics and attempt to save stats.

## Terminal controls

Both applications reuse the Windows English TUI and optional VFR1 manual repair flow.
Interactive stdin/stdout select TUI by default; `--no-tui` forces script-friendly CLI,
and `--tui` requires both terminals. The sender terminal supports R to enter/preview/
apply a request, Space to pause, X to cancel unsent repairs, Q/Ctrl+C to quit. The
receiver supports group selection, R to freeze requests and P for the copy view.
See [README controls](../README.md). Capture continues while reading a request; verified
completion remains in TUI until Q, whereas ordinary CLI exits automatically.

[Kitty/Hyprland TUI validation](benchmarks/linux-tui-20261016.md) passes a SHA-verified
1 MiB transfer with default TUI selection, pause/resume, resize recovery, Unicode/paste
editing, request apply/cancel/duplicate handling, copy view and graceful exit restoration.
Completion publishes final verified byte/hash/group counters and keeps the output path in
scrollable result text, including when returning to the frozen request panel. Closing the
sender editor clears stale preview notices. Physical-keyboard and broader TUI acceptance remain.

Keep the terminal away from the visible frame. Raw mode, alternate screen and cursor
are restored on ordinary errors/unwind; forced termination cannot promise restoration.
No clipboard, shared filesystem, network connection between endpoints or feedback is
required by the transport. D-Bus/PipeWire are local capture APIs only.

## Verification and desktop checkpoint

Headless regression coverage includes both profiles, repair-only late joining across
multiple generations, dropped/reordered/duplicate captures, padded RGB conversion,
negative stride and malformed buffer bounds, moved/resized frames, foreign sessions,
empty files, resource limits and create-new verified output/preview behavior. Callback
panic containment and cancellation/timeout polling are tested without opening a portal.
The existing workspace checks and release build remain required.

On 2026-10-16, `python3 scripts/check.py` (Python regressions, fmt/check/strict Clippy/
workspace tests), `cargo build --release --workspace`, the five-core no-default-features
check and `cargo +1.85.0 check --workspace --all-targets --locked` pass on Linux.
The main compiler is Rust 1.94.0; native headers are PipeWire 1.6.9 with libclang 23.1.
These are compilation and synthetic recovery results, not desktop evidence.

Real desktop testing must be explicitly cleared by the user before launching either
application against a desktop. The user cleared the following 1 KiB/1 MiB run on
2026-10-16; both pass independent SHA-256 on Hyprland 0.56.2. See the
[configuration, timing, hashes and remaining acceptance](benchmarks/local-hyprland-20261016.md):

```sh
python3 scripts/wayland-e2e.py --sizes 1024 1048576 --fps 30 --cell-size 4 --skip-build
# Additional acceptance: robust, late joining, empty file, move/cover/reveal,
# fractional/mixed DPI, manual rectangle, portal denial/revocation, TUI controls.
python3 scripts/wayland-e2e.py --sizes 1024 --profile robust --join-delay-seconds 5 --skip-build
```

The harness opens the sender and then the portal; the user selects a window/monitor
for each size. It creates unique evidence under ignored `output/`, preserves failed
logs and previews, compares input/output SHA-256 independently, and stops only children
it started. Unknown CPU/memory/display FPS fields stay blank. Movement/occlusion and
broader TUI interaction need additional testing. The recorded passes establish local Wayland
evidence for that configuration; cross-platform screen transfer and real remote acceptance remain
separate from compilation and synthetic recovery. Existing M14 limitations are unchanged.

References: [ScreenCast portal specification](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html),
[pipewire-rs examples](https://gitlab.freedesktop.org/pipewire/pipewire-rs/-/tree/0.9.2/pipewire/examples),
[softbuffer example](https://github.com/rust-windowing/softbuffer/blob/v0.4.8/examples/winit.rs).
