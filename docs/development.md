# Development on Windows and Linux

The repository's development entry points use Python 3.9+ and its standard library.
No pip packages or PowerShell installation are required. Rust 1.99.1 is the minimum supported
version and the pinned toolchain in `rust-toolchain.toml`; rustup installs rustfmt, Clippy and
`wasm32-unknown-unknown` with it. Browser checks also need Node.js 22+. On Linux, substitute `python3`
for `python` when necessary. Script paths below are relative to the repository root; scripts
resolve their working directory internally, so absolute script paths also work elsewhere.

## Windows

Native applications target Windows 10/11 x64. For `x86_64-pc-windows-msvc`, install Visual Studio
or its Build Tools with the C++ desktop workload and a Windows SDK. These supply the linker and
Windows import libraries; installing Rust alone is insufficient. See the official
[Rust MSVC prerequisites](https://rust-lang.github.io/rustup/installation/windows-msvc.html).

When copying a default MSVC release executable to another machine, provide the matching x64
[Visual C++ Redistributable](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist).
Its runtime version must be at least as recent as the build tools used. A fresh VM in the
[recorded RDP lab](benchmarks/remote-rdp-lab-20261002.md) failed with `0xC0000135` until a separate
static-CRT sender build was used. Static CRT is an explicit build choice, not the workspace default;
record build flags and executable hashes when distributing or measuring such a build.

PowerShell commands must include the executable path unless it is on PATH: use
`.\target\release\vft-send-win.exe` from the repository root, or `.\vft-send-win.exe` from a
directory containing a copied binary. See [PowerShell command resolution](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_command_precedence).

## Commands

| Python command | Purpose | Platforms |
| --- | --- | --- |
| `python scripts/check.py` | Script regression tests, Rust fmt/check/clippy/test; stops at first failure | Windows, Linux |
| `python scripts/build-web.py` | Self-contained `output/web-sender/index.html` with release WASM; `--debug` selects debug WASM | Windows, Linux |
| `python scripts/check-web.py` | Single-file build, WASM lint, source/bundle JS syntax and embedded Worker/WASM/native SHA-verified recovery | Windows, Linux |
| `python scripts/wayland-e2e.py` | Interactive portal selection, local Wayland transfer and independent SHA-256 | Linux Wayland desktop |
| `python scripts/local-e2e.py` | Local screen capture and independent output hash comparison | Windows desktop |
| `python scripts/check-tui-cli.py` | Six redirected CLI/TUI probes | Windows desktop |
| `python scripts/remote-benchmark.py` | Manual-rectangle remote capture and benchmark evidence | Windows desktop |

Each entry point supports `--help`. Arguments use lowercase kebab case, such as `--skip-build`
and `--cell-size`; `--sizes` takes space-separated integers. Cargo's configured target directory,
including `CARGO_TARGET_DIR`, is resolved through
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
the [native Wayland applications](linux-wayland.md). Building the workspace on Linux needs
PipeWire/SPA development packages, pkg-config, a C compiler and libclang. Windows desktop
scripts still fail before building or creating evidence on Linux; use `wayland-e2e.py` for
the Linux adapter. It opens a real window and ScreenCast picker, where the capture source must
be selected interactively. [Local Hyprland measurements](benchmarks/local-hyprland-20261016.md)
record SHA-verified 1 KiB/1 MiB transfers.
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
on failures and interruption. Windows local/remote launchers allow 30 additional seconds beyond
the receiver timeout; `wayland-e2e.py` allows 15 additional seconds.
Windows process measurements use ctypes; unavailable measurements remain blank. Movement targets
only a visible window belonging to the test-owned sender, using physical pixel coordinates.

The Windows sender's `--rect-file PATH` reserves a new file before opening its window.
Existing files (including input-file aliases) are rejected. The sender retains the file handle
and updates it only when the painted client rectangle changes. The file is initially empty;
readers must wait for a complete newline-terminated `X Y WIDTH HEIGHT` record, not just file
existence. The local harness waits for that record before starting capture.

Both `local-e2e.py` and `wayland-e2e.py` accept `--output-mode explicit|metadata|current-dir`
(default: `explicit`). Metadata mode creates an isolated receiving directory and supplies
`--output-dir` without `--output`; current-dir mode launches the receiver in that directory
with neither output option. Each mode independently compares the recovered file's SHA-256
with the source. The Linux harness requires interactive portal selection.

New local fixtures use `VFT-python-fixture-v1`: for each 64 KiB chunk starting at byte offset N,
take the required bytes from SHAKE-256 of ASCII `VFT-python-fixture-v1:N`. This is deterministic
across Windows and Linux, bounded to one chunk, and intentionally differs from historical
`.NET Random(20260930)` fixtures. Historical recorded hashes and timings remain unchanged;
new runs independently compare input/output SHA-256. `--join-delay-seconds` can test late joining.
Remote benchmarks accept an optional `--expected-sha256` from the sender for independent comparison;
without it, the receiver still verifies the manifest SHA, but the harness lacks a separate source hash.

## Validation boundaries

See [dependencies](dependencies.md) for the dated toolchain-baseline checks and
[measurements](benchmarks.md) for physical capture results. Running a remote benchmark launcher
against a local sender tests the launcher, not a remote transport. Workspace, headless browser,
local desktop and real remote results must be recorded separately. Preserve failed-run diagnostics;
verified goodput is zero when recovery fails. Desktop capture requires a visible, unobscured frame.
