# Development on Windows and Linux

The repository's development entry points use Python 3.9+ and its standard library.
No pip packages or PowerShell installation are required. Install Rust/Cargo; browser checks also
need the `wasm32-unknown-unknown` Rust target and Node.js 22+. On Linux, substitute `python3`
for `python` when necessary. Script paths below are relative to the repository root; scripts
resolve their working directory internally, so absolute script paths also work elsewhere.

| Python command | Purpose | Platforms |
| --- | --- | --- |
| `python scripts/check.py` | Script regression tests, Rust fmt/check/clippy/test; stops at first failure | Windows, Linux |
| `python scripts/build-web.py` | Self-contained `output/web-sender/index.html` with release WASM; `--debug` selects debug WASM | Windows, Linux |
| `python scripts/check-web.py` | Single-file build, WASM lint, source/bundle JS syntax and embedded Worker/WASM/native SHA-verified recovery | Windows, Linux |
| `python scripts/wayland-e2e.py` | Interactive portal selection, local Wayland transfer and independent SHA-256 | Linux Wayland desktop |
| `python scripts/local-e2e.py` | Local screen capture and independent output hash comparison | Windows desktop |
| `python scripts/check-tui-cli.py` | Six redirected CLI/TUI probes | Windows desktop |
| `python scripts/remote-benchmark.py` | Manual-rectangle remote capture and benchmark evidence | Windows desktop |

Each entry point supports `--help`. The former `.ps1` entry points have been replaced; arguments
now use lowercase kebab case (`-SkipBuild` becomes `--skip-build`, `-CellSize` becomes
`--cell-size`). `--sizes` takes space-separated integers instead of a PowerShell comma-separated
array. Cargo's configured target directory, including `CARGO_TARGET_DIR`, is resolved through
`cargo metadata`. Relative Cargo configuration is evaluated from the repository root.
Historical benchmark records retain the PowerShell commands actually used for those measurements.

```text
cargo fmt --all
python scripts/check.py
rustup target add wasm32-unknown-unknown
python scripts/check-web.py
```

Open `output/web-sender/index.html` directly in a browser; it needs no local server or CORS
exceptions. The HTML embeds all assets and runs a classic Blob Worker. An ordinary HTTP/HTTPS
static host remains an option. See [browser requirements and packaging](wasm-sender.md).

Linux development supports the portable Rust cores, headless tests, browser distribution and
the [native Wayland extension](linux-wayland.md). Building the workspace on Linux now needs
PipeWire/SPA development packages, pkg-config, a C compiler and libclang. Windows desktop
scripts still fail before building or creating evidence on Linux; use `wayland-e2e.py` for
the Linux adapter. It opens a real window and ScreenCast picker and requires user clearance
before agent-driven desktop testing. Its user-approved
[local Hyprland 1 KiB/1 MiB run](benchmarks/local-hyprland-20261016.md) passes SHA verification.
A WASM/native headless pass does not establish browser screen capture or real remote acceptance.

On an unlocked Windows desktop:

```text
python scripts/local-e2e.py --sizes 1024 1048576 --fps 60 --cell-size 4
python scripts/local-e2e.py --sizes 1048576 --fps 60 --cell-size 4 --auto-locate --move-sender
python scripts/check-tui-cli.py --skip-build
# Metadata filename in a selected directory, then in the receiver's working directory:
python scripts/local-e2e.py --sizes 1024 1048576 --fps 60 --cell-size 4 --output-mode metadata --skip-build
python scripts/local-e2e.py --sizes 1024 --fps 60 --cell-size 4 --output-mode current-dir --skip-build
```

Native harnesses create unique directories under ignored `output/`, use create-new fixtures,
logs and result files, and close only the child processes they launch. They retain diagnostics
on failures and interruption. Receiver timeouts have an additional 30-second launcher allowance.
Windows process measurements use ctypes; unavailable measurements remain blank. Movement targets
only a visible window belonging to the test-owned sender, using physical pixel coordinates.

Both `local-e2e.py` and `wayland-e2e.py` accept `--output-mode explicit|metadata|current-dir`
(default: `explicit`). Metadata mode creates an isolated receiving directory and supplies
`--output-dir` without `--output`; current-dir mode launches the receiver in that directory
with neither output option. Each mode independently compares the recovered file's SHA-256
with the source. The Linux harness still requires user clearance and portal selection.

New local fixtures use `VFT-python-fixture-v1`: for each 64 KiB chunk starting at byte offset N,
take the required bytes from SHAKE-256 of ASCII `VFT-python-fixture-v1:N`. This is deterministic
across Windows and Linux, bounded to one chunk, and intentionally differs from historical
`.NET Random(20260930)` fixtures. Historical recorded hashes and timings remain unchanged;
new runs independently compare input/output SHA-256. `--join-delay-seconds` can test late joining.
Remote benchmarks accept an optional `--expected-sha256` from the sender for independent comparison;
without it, the receiver still verifies the manifest SHA, but the harness lacks a separate source hash.

## Migration verification (2026-10-02)

`check.py` and `check-web.py` pass on Windows x64 (Rust 1.98.1, Python 3.14.7, Node 24.19.0)
and Arch Linux under WSL (Rust 1.95.0, Python 3.14.7, Node 26.10.0). Linux builds used
`CARGO_TARGET_DIR=.build/linux-target` to avoid sharing native build artifacts with Windows.
The Linux WASM check first failed with the target missing; installing it with
`rustup target add wasm32-unknown-unknown` allowed the complete check to pass. Python regressions
also pass on Ubuntu with Python 3.12.3. Both debug and release web builds pass on Windows.

Windows release desktop checks independently match SHA-256 for balanced 1 KiB/1 MiB transfers,
1 MiB auto-location with movement and reacquisition, robust manual-crop empty/1 KiB transfers,
and a 1 KiB join delayed by two seconds (zero captured source frames). All six redirected CLI/TUI
probes pass. `remote-benchmark.py` also passes with a local sender, manual rectangle and supplied
independent source hash, with transport labeled `local-launcher-smoke`. This is a launcher
regression check, not a real remote benchmark. These checks add no browser-to-Windows physical
capture or remote acceptance evidence.
