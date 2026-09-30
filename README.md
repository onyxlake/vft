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

For remote desktop capture, replace `--sender-window` with `--rect X Y WIDTH HEIGHT` in physical
screen pixels, containing only the visual frame. ESC exits the sender, SPACE pauses and F11 toggles
fullscreen. Robust defaults to 16-pixel binary cells; balanced uses 8-pixel four-level cells.
Keep the sender visible and the source file unchanged. The receiver refuses to overwrite output.

```powershell
# Native screen tests; creates unique fixtures/logs under output/ and closes its own processes.
./scripts/local-e2e.ps1 -Sizes 1024,1048576,104857600 -Fps 60 -CellSize 4
```

An unlocked interactive Windows desktop is required for capture. Sender reads files in bounded
chunks; the initial receiver retains recovered data in memory, with a 256 MiB default limit.
No direct network connection, acknowledgements or feedback files are required.

See [protocol](docs/protocol.md), [visual format](docs/visual-format.md), [architecture](docs/architecture.md),
[milestone status](docs/status.md), [benchmarks](docs/benchmarks.md),
[real remote validation](docs/remote-validation.md), and [future platforms](docs/future-platforms.md).
Remote desktop performance and Phase 1 completion require real remote tests; local simulation is
not sufficient evidence.
