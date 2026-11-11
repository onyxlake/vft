# Build and test guide

The repository's development entry points use Python 3.9+ and its standard library.
No pip packages or PowerShell installation are required. Rust 1.99.1 is the minimum supported
version and the pinned toolchain in `rust-toolchain.toml`; the workspace uses edition 2021.
`Cargo.toml` declares dependencies and `Cargo.lock` pins their resolved versions. Rustup installs
rustfmt, Clippy and `wasm32-unknown-unknown` with it. Browser checks also need Node.js 22+. On Linux, substitute `python3`
for `python` when necessary. Script paths below are relative to the repository root; scripts
resolve their working directory internally, so absolute script paths also work elsewhere.

## Windows

Native applications target Windows 10/11 x64. For `x86_64-pc-windows-msvc`, install Visual Studio
or its Build Tools with the C++ desktop workload and a Windows SDK. These supply the linker and
Windows import libraries; installing Rust alone is insufficient. See the official
[Rust MSVC prerequisites](https://rust-lang.github.io/rustup/installation/windows-msvc.html).

When copying a default MSVC release executable to another machine, provide the matching x64
[Visual C++ Redistributable](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist).
Its runtime version must be at least as recent as the build tools used. Default MSVC builds
use the dynamic CRT; static CRT requires an explicit build configuration.

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
| `python scripts/remote-benchmark.py` | Manual-rectangle remote capture and diagnostics | Windows desktop |

Each entry point supports `--help`. Arguments use lowercase kebab case, such as `--skip-build`
and `--cell-size`; `--sizes` takes space-separated integers. Cargo's configured target directory,
including `CARGO_TARGET_DIR`, is resolved through `cargo metadata`. Relative Cargo configuration
is evaluated from the repository root.

```text
cargo fmt --all
python scripts/check.py
rustup target add wasm32-unknown-unknown
python scripts/check-web.py
git diff --check
```

`check-web.py` requires Node.js 22+. It builds and lints the WASM adapter, checks source/bundled
JavaScript syntax, and runs the embedded Worker/WASM in Node with a bounded FileReaderSync stand-in.
The native visual/receiver cores render, sample and recover its frames, then independently compare
bytes and SHA-256. This is a headless check; browser UI and physical capture need separate testing.
See [browser requirements and packaging](wasm-sender.md) for distributing the generated HTML.

## GitHub CI and releases

The [CI workflow](../.github/workflows/ci.yml) runs on pull requests, pushes to `master`,
`v*` tags and manual dispatch. It uses the exact toolchain in `rust-toolchain.toml`.
Windows Server 2022 x64 and Ubuntu 24.04 x64 runners execute `scripts/check.py`, check the
shared cores without default features and build the workspace in release mode. A separate
Ubuntu job checks the cores on WASM and runs `scripts/check-web.py` with Node.js 22.
These are build and headless checks; they do not exercise desktop capture, portal selection,
browser UI or real remote transfer.

Successful jobs retain these downloadable Actions artifacts for 14 days:

| Artifact | Contents |
| --- | --- |
| `vft-windows-x86_64` | `vft-windows-x86_64.zip`: `vft-send-win.exe`, `vft-recv-win.exe`, README, license and technical guides |
| `vft-linux-x86_64` | `vft-linux-x86_64.tar.gz`: `vft-send-linux`, `vft-recv-linux`, README, license and technical guides; executable permissions preserved |
| `vft-web` | `index.html`, raw `vft_send_web.wasm` and `web-THIRD-PARTY-NOTICES.txt` |

`index.html` is self-contained; open it directly in a browser. It already embeds WASM and
license notices. The separate WASM file is
for integrations using the [documented ABI](wasm-sender.md#adapter-and-abi); retain its companion
notices when redistributing it. Native packages still need the documented Windows VC++ runtime
or [Linux runtime dependencies](linux-wayland.md#build-and-runtime-requirements). Linux binaries
are built on Ubuntu 24.04 and are not static or universally portable to older distributions.

Master builds publish Actions artifacts only. Pushing a tag such as `v0.1.0` creates a GitHub
Release **after both native jobs and the web job pass**, attaching the two native archives,
the three web files and `SHA256SUMS.txt`. Tags containing a hyphen (for example `v0.1.0-rc.1`)
publish prereleases. Existing releases/assets are not overwritten; a rerun against an already
published tag fails. Only the release job receives `contents: write`; PR jobs use read-only
repository permissions and do not need secrets.

The workflows require a GitHub repository with Actions enabled; committing them to a different
Git host does not activate GitHub Actions. Set `master` as the GitHub default branch.
[Dependabot](../.github/dependabot.yml) checks Cargo and pinned GitHub Actions every Monday at
09:00 Asia/Shanghai and opens PRs against the default branch. Cargo minor/patch updates are
grouped, major updates remain separate, and Actions updates form one group. Updates run the
same PR checks and require normal review; automatic merging is not configured. To require CI
before merging, add `Native (windows-2022)`, `Native (ubuntu-24.04)` and `WASM and web` as required
checks in the repository's branch rules.

## Desktop tests

Linux development supports the portable Rust cores, headless tests, browser distribution and
the [native Wayland applications](linux-wayland.md). Building the workspace on Linux needs
PipeWire/SPA development packages, pkg-config, a C compiler and libclang. Windows desktop
scripts fail before building or creating output on Linux. Use `wayland-e2e.py` in a Wayland
desktop session and select the sender window or monitor in the portal for each file size:

```sh
python3 scripts/wayland-e2e.py --sizes 1024 1048576 --fps 30 --cell-size 4
```

On an unlocked Windows desktop:

```text
python scripts/local-e2e.py --sizes 1024 1048576 --fps 60 --cell-size 4
python scripts/local-e2e.py --sizes 1048576 --fps 60 --cell-size 4 --auto-locate --move-sender
python scripts/check-tui-cli.py --skip-build
```

Native harnesses create unique directories under ignored `output/`, use create-new fixtures,
logs and result files, and close only the child processes they launch. They retain diagnostics
on failures and interruption. Windows local/remote launchers allow 30 additional seconds beyond
the receiver timeout; `wayland-e2e.py` allows 15 additional seconds.
Windows process measurements use ctypes; unavailable measurements remain blank. Movement targets
only a visible window belonging to the test-owned sender, using physical pixel coordinates.

The Windows local harness waits for a complete newline-terminated `--rect-file` record before
starting capture; see the [sender CLI reference](cli.md#sender) for its create-new semantics.

Both `local-e2e.py` and `wayland-e2e.py` accept `--output-mode explicit|metadata|current-dir`
(default: `explicit`). Metadata mode creates an isolated receiving directory and supplies
`--output-dir` without `--output`; current-dir mode launches the receiver in that directory
with neither output option. Each mode independently compares the recovered file's SHA-256
with the source. The Linux harness requires interactive portal selection.

Local fixtures use `VFT-python-fixture-v1`: for each 64 KiB chunk starting at byte offset N,
take the required bytes from SHAKE-256 of ASCII `VFT-python-fixture-v1:N`. This is deterministic
across Windows and Linux and bounded to one chunk. `--join-delay-seconds` can test late joining.
Desktop capture requires an unlocked session and a visible, unobscured frame. Use `--topmost`
for the Windows sender where needed; overlays can still obscure it.

For a remote Windows sender, start the sender on that machine and run `remote-benchmark.py`
locally with the exact frame rectangle. For example, replace these coordinates and the hash
with values for the displayed frame and original file:

```powershell
python scripts/remote-benchmark.py --x 50 --y 80 --width 1536 --height 864 --profile balanced --sender-fps 15 --cell-size 8 --transport RDP --codec unknown --expected-sha256 THE_HASH_FROM_THE_SENDER
```

The script does not communicate with the sender. `--expected-sha256` supplies an independent
source hash; omitting it still checks the manifest SHA but provides no separate source comparison.
Generated CSVs, configuration, logs and previews stay under ignored `output/`.
Verified goodput is original bytes divided by receiver elapsed time and is zero on failure.
Requested FPS, duplicate captures and decode rejections do not measure video frame loss.
