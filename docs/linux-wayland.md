# Linux Wayland sender and receiver

`vft-send-linux` and `vft-recv-linux` use the shared v0 protocol, FEC, visual codec and
sender/receiver sessions. They run on Wayland; X11 is not implemented.

## Build and runtime requirements

Rust 1.99.1 or newer, a C compiler, pkg-config, libclang (for PipeWire bindgen),
PipeWire/SPA development headers and libraries are needed to build the Linux receiver.
The sender dynamically loads Wayland and xkbcommon; it uses winit 0.30.13 and
softbuffer 0.4.8 with only the Wayland backend enabled. The receiver uses ashpd 0.14.0
(Tokio and ScreenCast features) and pipewire-rs 0.10.1. Linux desktop dependencies are
target-specific and do not enter the five `no_std + alloc` cores or Windows builds.

For example, Arch Linux build packages are `base-devel pkgconf clang pipewire`;
Debian/Ubuntu equivalents include `build-essential pkg-config libclang-dev
libpipewire-0.3-dev libspa-0.2-dev`. Runtime needs a running Wayland session, a session
D-Bus, PipeWire, `xdg-desktop-portal`, and a compositor-appropriate portal backend
providing ScreenCast (for example GNOME, KDE or a supported wlroots backend).
Install/configure the backend for your desktop; the applications do not change services.

```sh
cargo build --release -p vft-send-linux -p vft-recv-linux
mkdir -p output
# Run the sender and receiver in separate terminals:
./target/release/vft-send-linux input.bin --profile balanced --fps 15 --cell-size 4
./target/release/vft-recv-linux --auto --output recovered.bin --preview output/crop.bmp
```

Use `cargo metadata` to locate binaries if Cargo uses a custom target directory.
Diagnostic directories must already exist. Output, stats and preview paths must be
new and distinct; no file is silently overwritten. All filenames printed in terminal
controls are escaped for control characters. See the [CLI reference](cli.md) for all options.

`--output FILE` is optional. Without it, the receiver saves the original manifest filename
in `--output-dir DIR`, which defaults to the current working directory and must already
exist. An explicit `--output` always uses its supplied path and overrides `--output-dir`;
a relative explicit path stays relative to the working directory. The resolved destination
appears in CLI/TUI progress and completion. Unsafe metadata filenames, existing destinations
and collisions with preview/statistics files are rejected. Output uses create-new semantics
after full-file SHA-256 verification.

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
remains memory-backed with a default **256 MiB file limit** and a separate **1,000,000 source-symbol
limit**. `--max-file-size` changes only the byte limit; see [resource limits](cli.md#resource-limits).

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

Both applications use the shared [terminal controls and optional manual repair](terminal-controls.md).
Interactive stdin/stdout select TUI by default; `--no-tui` selects ordinary CLI. Capture continues
while reading a request, and verified TUI completion remains visible until Quit.
Keep the terminal outside the visual frame. D-Bus/PipeWire are local capture APIs; no network
connection or feedback channel between endpoints is required.

The [build and test guide](development.md) covers workspace checks and the interactive
`wayland-e2e.py` harness.

References: [ScreenCast portal specification](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html),
[pipewire-rs examples](https://gitlab.freedesktop.org/pipewire/pipewire-rs/-/tree/0.10.1/pipewire/examples),
[softbuffer example](https://github.com/rust-windowing/softbuffer/blob/v0.4.8/examples/winit.rs).
