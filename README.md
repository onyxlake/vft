# VFT — Visual File Transfer

Simplex file transfer over independent luminance frames. Rust shared libraries, native Windows
sender/receiver, Linux Wayland sender/receiver and a browser WASM sender; no network transport
or required acknowledgements.
See [the implementation plan](docs/plan.md).

Windows controls now support [manual repair requests and TUI](docs/manual-repair-tui-plan.md).
Interactive stdin/stdout default to the English TUI; `--no-tui` forces ordinary CLI for scripts.
Explicit `--tui` requires interactive stdin and stdout. The visual transport stays in its own window.

Development scripts require Python 3.9+ and Rust 1.99.1 or newer, with no Python packages to install.
`rust-toolchain.toml` pins Rust 1.99.1 with rustfmt, Clippy and the WASM target for reproducible checks.
Use `python` on Windows or `python3` on Linux if `python` is unavailable. Commands work from any
working directory. Workspace and browser checks run on both systems; native desktop checks require
an unlocked interactive Windows or Linux Wayland desktop, using the platform-specific harness.
Linux workspace builds also need the [PipeWire development dependencies](docs/linux-wayland.md).
See [development scripts](docs/development.md).

Build and validate on Windows or Linux:

```powershell
python scripts/check.py
```

The six crates implement protocol v0, deterministic FEC, calibrated binary/four-level visual
frames, simplex sender/receiver cores and a seeded distortion simulator. The two Windows apps use
Win32/GDI with software framebuffers and desktop region capture. The browser adapter reuses the
sender and visual cores in a Worker and displays frames on Canvas. Shared cores are `no_std + alloc`.

```powershell
cargo build --release --workspace
./target/release/vft-send-win.exe input.bin --profile balanced --fps 15 --topmost
./target/release/vft-recv-win.exe --sender-window --output recovered.bin
```

For remote desktop capture, use `--auto` to locate the visible VFT frame on your local desktop:

```powershell
./target/release/vft-recv-win.exe --auto --output recovered.bin --preview crop.bmp
# Optionally restrict discovery to a monitor or the remote desktop window, in physical pixels:
./target/release/vft-recv-win.exe --auto --search-rect 0 0 1920 1080 --output recovered.bin
```

Automatic discovery uses the existing sync bands and requires a CRC-valid packet before locking.
It searches about twice per second, then captures only the located rectangle at `--fps` (default
30). After 750 ms of continuous decoding failure it searches again, retaining recovery progress
and the selected session. A moved/resized or temporarily covered frame can therefore be reacquired.
`--preview` saves the first located crop, rather than a full desktop screenshot.

Keep the complete frame visible and axis-aligned, with at least two physical pixels per cell.
Discovery supports the two standard grids; rotated/perspective camera images are unsupported.
The search rectangle is limited to 8192 pixels per dimension. Use `--search-rect` when a combined
multi-monitor desktop exceeds this limit or several VFT senders are visible. Without a selected
manifest, the first discovered valid frame determines the initial capture location.

Manual `--rect X Y WIDTH HEIGHT` remains available: it must contain only the complete visual frame.
`--auto`, `--rect` and `--sender-window` are mutually exclusive. ESC exits the sender, SPACE pauses and F11 toggles
fullscreen. Robust defaults to 16-pixel binary cells; balanced uses 8-pixel four-level cells.
Keep the sender visible and the source file unchanged. The receiver refuses to overwrite output.

Both native receivers accept optional `--output-dir DIR` (default: the current working
directory). Without `--output`, they save using the original filename in the manifest.
The selected directory must already exist. `--output FILE` takes precedence and uses its
exact supplied path, even when `--output-dir` is also present; relative explicit paths remain
relative to the working directory. For example, if the manifest names `photo.png`:

| Receiver options | Destination |
| --- | --- |
| `--auto` | `./photo.png` |
| `--auto --output-dir ./received` | `./received/photo.png` |
| `--auto --output renamed.png` | `./renamed.png` |
| `--auto --output-dir ./received --output renamed.png` | `./renamed.png` |

The receiver shows the destination after accepting the manifest and on completion. Unsafe
metadata filenames, existing destinations and collisions with preview/statistics paths are
rejected; Windows also rejects device names, invalid characters and trailing dots/spaces.
Use `--output` to choose a local name when needed. Recovered bytes are written only after
SHA-256 verification, using create-new semantics. For a local sender and receiver sharing
a directory, choose a different output directory or an explicit new filename.
CLI and TUI logs escape control characters in transmitted filenames and other metadata,
including when `--output` supplies a different local filename. The transmitted metadata itself
is unchanged.

In the receiver TUI, use Up/Down and PgUp/PgDn to inspect incomplete groups (numbers start at 1).
Space selects a group, A selects the page, and C clears selection. R freezes a request snapshot
for selected groups, or all incomplete groups if none are selected. Capture continues while you
read it. Tab switches between table and request; Left/Right switch request-code pages, and
Up/Down or PgUp/PgDn scroll frozen text. R generates a fresh snapshot; each code has at most
256 targets. P opens a copy view that prints the full code on one logical line with terminal
wrapping; capture continues and Tab returns to the TUI. Manually select/copy the code or transcribe
groups to the sender; clipboard sharing is optional.

In the sender TUI, R opens the input editor. Enter validates and previews; Enter again applies.
Examples: `18,42,90-91` sends sixteen new repairs per group; `18:3,42:1` describes rank deficits
and computes a repair budget. Full `VFR1/...` codes check the current manifest tag and CRC, and
repeated accepted IDs are no-ops. Check the file and tag when using simple lists. One manual batch
can be active at a time; X cancels its unsent budget. Automatic transmission continues between
manual batches. Space pauses/resumes, Q or Ctrl+C exits; while editing, shortcuts are text and
Esc closes the editor. The visual window's ESC/SPACE/F11 controls still work.

Use at least an 80x24 terminal (100x28 provides more space) outside the visible frame. Verified
receiver completion remains visible until Q, including the output path; ordinary CLI still exits
automatically. `Manual repair batch sent` means only that packets were sent. Manual feedback
remains optional and recovery still works without it. [Local validation and remaining acceptance
work](docs/benchmarks/manual-repair-tui-20261002.md) do not establish remote performance improvement.

```powershell
# Native screen tests; creates unique fixtures/logs under output/ and closes its own processes.
python scripts/local-e2e.py --sizes 1024 1048576 104857600 --fps 60 --cell-size 4
# Auto-discovery and movement/reacquisition test:
python scripts/local-e2e.py --sizes 1048576 --fps 60 --cell-size 4 --auto-locate --move-sender
# Redirected CLI / explicit TUI mode probes:
python scripts/check-tui-cli.py
```

An unlocked interactive Windows desktop is required for capture. Sender reads files in bounded
chunks; the initial receiver retains recovered data in memory, with a 256 MiB default limit.
No direct network connection, acknowledgements or feedback files are required.

Linux Wayland (see [dependencies, capture coordinates and validation limits](docs/linux-wayland.md)):

```sh
cargo build --release -p vft-send-linux -p vft-recv-linux
./target/release/vft-send-linux input.bin --fps 15 --cell-size 4
./target/release/vft-recv-linux --auto --output recovered.bin
```

Select the sender window or monitor in the ScreenCast portal picker. Linux uses native
Wayland software presentation and portal/PipeWire capture, with the same optional TUI
and manual repair controls. `--rect`/`--search-rect` use selected-stream physical pixels.
X11 and DMA-BUF-only capture are unsupported. [Local Hyprland 1 KiB/1 MiB transfers](docs/benchmarks/local-hyprland-20261016.md)
pass independent SHA-256 verification. [Kitty TUI controls](docs/benchmarks/linux-tui-20261016.md)
also pass a 1 MiB transfer with manual requests and terminal restoration; broader desktop
scenarios and remote acceptance remain.

Browser sender (build tools are needed only to create the static distribution):

```powershell
rustup target add wasm32-unknown-unknown
python scripts/build-web.py
# Open output/web-sender/index.html directly in your browser; select a file and start sending.
# On the receiving Windows desktop:
./target/release/vft-recv-win.exe --auto --output recovered.bin
```

The build produces one self-contained `index.html` with an English demo UI, embedded JavaScript,
CSS and WASM. It runs directly through `file://` without a local server or external assets;
HTTP/HTTPS hosting also works. No file bytes are uploaded. The browser hashes
in 64 KiB slices and reads symbols on demand. Choose balanced/robust, FPS and physical pixels per
cell, then keep the full Canvas visible; fullscreen provides more room. Small windows reduce
cell size to fit (minimum 2 physical pixels). Hiding the tab pauses transmission; return and click
Resume. Stop sending after the receiver reports SHA-verified completion.

See [browser sender details and validation](docs/wasm-sender.md). With Node.js 22+ installed,
`python scripts/check-web.py` additionally verifies the compiled WASM against the native decoder,
including loss, duplicates, reordering and repair-only joining. This is separate from browser
screen capture and real remote desktop acceptance.

See [protocol](docs/protocol.md), [visual format](docs/visual-format.md), [architecture](docs/architecture.md),
[milestone status](docs/status.md), [benchmarks](docs/benchmarks.md),
[real remote validation](docs/remote-validation.md), and [future platforms](docs/future-platforms.md).
Remote desktop performance and Phase 1 completion require real remote tests; local simulation is
not sufficient evidence.
