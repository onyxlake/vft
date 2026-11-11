# VFT — Visual File Transfer

VFT transfers a file through independent luminance frames displayed on a screen. A receiver
captures the frames, recovers missing data with deterministic FEC, and saves the file after
SHA-256 verification. The transfer requires no network connection between endpoints, shared
filesystem, acknowledgements or capture-order continuity.

Native sender/receiver applications are available for Windows 10/11 x64 and Linux Wayland.
A browser sender runs from a self-contained HTML file. The five shared Rust cores use
`no_std + alloc`; the current wire format is VFT v0.

Real RDP transfers have recovered 1 MiB and 100 MiB files. The sustained 100 MiB result was
29.3 KiB/s, below the 40–100 decimal KB/s target. Remote validation remains partial, and browser
physical capture and Linux remote acceptance remain unverified. See
[support and limitations](docs/status.md) and [measurements](docs/benchmarks.md).

## Build requirements

- Rust **1.99.1** or newer; `rust-toolchain.toml` pins 1.99.1, rustfmt, Clippy and the WASM target.
- Python **3.9+** for development scripts; no Python packages are required. Use `python3` on
  Linux when `python` is unavailable. Node.js **22+** is needed only for browser development tests.
- Windows MSVC builds need the C++ build tools and Windows SDK. Copied MSVC executables normally
  need the matching x64 Visual C++ runtime. See [Windows setup](docs/development.md#windows).
- Linux workspace builds need PipeWire/SPA development libraries, pkg-config, a C compiler and
  libclang. Runtime capture needs Wayland, PipeWire and a ScreenCast portal backend. See
  [Linux requirements](docs/linux-wayland.md#build-and-runtime-requirements).

Run the commands below from the repository root. Cargo's default binary directory is shown;
use `cargo metadata` to find it if you configured a different target directory.

## Windows

Build on Windows:

```powershell
cargo build --release -p vft-send-win -p vft-recv-win
```

In separate terminals, start the sender and receiver:

```powershell
.\target\release\vft-send-win.exe input.bin --profile balanced --fps 15 --topmost
.\target\release\vft-recv-win.exe --auto --output recovered.bin
```

For remote desktop use, run the sender on the remote machine and the receiver on the machine
showing that desktop. `--auto` finds the visible frame and reacquires it after movement.
Keep the complete frame visible. For a local native sender, `--sender-window` can follow its
client area; `--rect X Y WIDTH HEIGHT` accepts an exact physical-pixel crop.

## Linux Wayland

After installing the [system dependencies](docs/linux-wayland.md):

```sh
cargo build --release -p vft-send-linux -p vft-recv-linux
# Run these in separate terminals:
./target/release/vft-send-linux input.bin --profile balanced --fps 15 --cell-size 4
./target/release/vft-recv-linux --auto --output recovered.bin
```

Select the sender window or its monitor in the ScreenCast picker. Linux rectangles use physical
pixels in the selected stream, with origin `(0, 0)`. X11 and DMA-BUF-only capture are unsupported.
Four-pixel cells keep this local example small; use the larger 8/16-pixel presets for remote
compression where space permits.

## Browser sender

Build the distribution once:

```sh
python scripts/build-web.py
```

Open `output/web-sender/index.html` directly in a browser, select a file and start sending.
Copying that single HTML file is sufficient; no server, external assets or upload is required.
Its embedded license notices must remain when redistributing it. Start a native receiver with
`--auto` on the receiving desktop. See [browser requirements and controls](docs/wasm-sender.md).

## Output and controls

Without `--output`, native receivers save the manifest filename in `--output-dir DIR`, defaulting
to the current directory. An explicit `--output FILE` takes precedence. Destination directories
must exist; output, preview and statistics files must be new and distinct. For a local transfer,
choose a new output name or a different directory so the source is not also the destination.

Receiver storage is memory-backed, with a default **256 MiB** file limit and a separate
**1,000,000 source-symbol** limit. Raising `--max-file-size` changes only the byte limit.
See [CLI options, output rules and resource limits](docs/cli.md).

Interactive terminals use the English TUI by default; `--no-tui` selects ordinary CLI output.
Space pauses the sender, Q or Ctrl+C exits the TUI, and verified receiver results stay visible
until Quit. Optional manual repair requests use R on both ends. The visual window has its own
Esc/Space/F11 controls and must remain clear of the terminal. See [terminal controls](docs/terminal-controls.md).

## Documentation

| Guide | Contents |
| --- | --- |
| [CLI reference](docs/cli.md) | Native arguments, capture modes, output paths and limits |
| [Terminal controls](docs/terminal-controls.md) | TUI navigation and optional manual repair |
| [Linux Wayland](docs/linux-wayland.md) | Dependencies, presentation and portal capture |
| [Browser sender](docs/wasm-sender.md) | Offline HTML distribution, lifecycle and WASM ABI |
| [Protocol](docs/protocol.md) / [visual format](docs/visual-format.md) | Wire encoding, FEC and exact luminance layout |
| [Architecture](docs/architecture.md) | Core and adapter responsibilities |
| [Development](docs/development.md) / [dependencies](docs/dependencies.md) | Build setup, checks, scripts and toolchain baseline |
| [Support and limitations](docs/status.md) | Implemented features and validation boundaries |
| [Measurements](docs/benchmarks.md) / [remote validation](docs/remote-validation.md) | Recorded results and reproducible measurement procedures |

Run `python scripts/check.py` for Python regressions and Rust format/check/Clippy/tests.
With Node.js 22+ installed, `python scripts/check-web.py` also checks packaged WASM/native recovery.
Desktop tests use separate platform-specific scripts described in the development guide.

VFT is licensed under the [MIT license](LICENSE).
