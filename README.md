# VFT — Visual File Transfer

Simplex file transfer over independent luminance frames. Rust shared libraries and native Windows
sender/receiver; no network transport or required acknowledgements. See [the implementation plan](docs/plan.md).

Build and validate:

```powershell
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The six crates implement protocol v0, deterministic FEC, calibrated binary/four-level visual
frames, simplex sender/receiver cores and a seeded distortion simulator. The two Windows apps use
Win32/GDI with software framebuffers and desktop region capture. Shared cores are `no_std + alloc`.

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

```powershell
# Native screen tests; creates unique fixtures/logs under output/ and closes its own processes.
./scripts/local-e2e.ps1 -Sizes 1024,1048576,104857600 -Fps 60 -CellSize 4
# Auto-discovery and movement/reacquisition test:
./scripts/local-e2e.ps1 -Sizes 1048576 -Fps 60 -CellSize 4 -AutoLocate -MoveSender
```

An unlocked interactive Windows desktop is required for capture. Sender reads files in bounded
chunks; the initial receiver retains recovered data in memory, with a 256 MiB default limit.
No direct network connection, acknowledgements or feedback files are required.

See [protocol](docs/protocol.md), [visual format](docs/visual-format.md), [architecture](docs/architecture.md),
[milestone status](docs/status.md), [benchmarks](docs/benchmarks.md),
[real remote validation](docs/remote-validation.md), and [future platforms](docs/future-platforms.md).
Remote desktop performance and Phase 1 completion require real remote tests; local simulation is
not sufficient evidence.
