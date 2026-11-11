# Implementation status

Updated 2026-11-11 (Asia/Shanghai). The original scope is preserved in [plan.md](plan.md).

The workspace now requires and pins Rust 1.99.1. Direct dependencies and the lockfile have been
updated; `lru 0.18.5` replaces the version with two reported soundness issues, and `paste` plus
the retired D-Bus companion crates are removed. An OSV query of all 304 registry packages in
the final lockfile returns no advisory matches. Linux checks (19 Python and 65 Rust tests),
core no-default-features/native/WASM checks, Windows cross-checks, Linux release builds and
WASM/native recovery pass, as do headless Chromium/Firefox offline checks. See
[versions, audit scope and validation limits](dependencies.md). Physical desktop and remote
capture were not rerun for this upgrade.
Rust 1.85 results below are historical evidence for earlier revisions, not the current baseline.

The browser sender now has a plain English demo UI and builds to one self-contained
`output/web-sender/index.html`. Embedded WASM and a classic Blob Worker allow direct `file://`
use without a server or external assets. Linux workspace checks, release builds, packaged
WASM/native recovery and headless Chromium/Firefox offline UI checks pass. See
[single-file demo validation](benchmarks/web-demo-20261111.md). Browser physical capture and
real remote acceptance remain unverified; no M14 status change is implied.

Both native receivers now accept optional `--output-dir DIR` (default: current working
directory). Without `--output`, the manifest supplies the saved filename. Explicit output
paths take precedence; directories must exist, unsafe names and diagnostic collisions are
rejected, and recovered output still requires SHA verification and create-new writes.
Windows/Linux workspace checks and release builds, plus Windows Rust 1.85 all-target
compilation, pass.
Windows 1 KiB/1 MiB physical captures pass independent SHA-256 in all three destination
modes; the six redirected CLI/TUI probes also pass. See [validation and measurements](benchmarks/receiver-output-20261109.md).
Linux desktop capture and real remote acceptance were not rerun for this change.

| Milestone | Status | Evidence |
| --- | --- | --- |
| M0 workspace | Complete | Thirteen workspace members including the browser adapter and shared TUI helper, format/lint setup and local Git checkpoints |
| M1 protocol v0 | Complete | Packet/manifest serialization, CRC32C, independent golden vectors |
| M2 block model | Complete | 64-symbol generations, zero padding, streaming random-access sender source |
| M3 deterministic FEC | Complete | Systematic/dense repair encoding; loss, duplicate, reorder, repair-only tests |
| M4 visual codec | Complete | Binary/four-level cells, bootstrap, sync, calibration, rendering and sampling |
| M5 simulator | Complete | Seeded drop/duplicate/corruption/blur/resize/brightness/contrast/noise/reorder |
| M6 sender core | Complete | Repeated manifests, sources once, continuing repairs, bounded file cache |
| M7 receiver core | Complete | Discovery, CRC, duplicate window, FEC rank/progress, SHA-verified completion |
| M8 headless E2E | Complete | Both profiles, distortion/loss/reorder, late joining, 1 MiB visual recovery |
| M9 Windows sender shell | Complete | Native window, software framebuffer, DIB painting, keyboard/fullscreen code |
| M10 sender integration | Complete | File CLI, profiles/FPS/cell size, bounded file reads, live progress title |
| M11 Windows capture | Complete | Physical screen rectangle or known local sender client area, diagnostic BMP |
| M12 Windows sampling | Complete | Known grid alignment, sync validation, luminance calibration and sampling |
| M13 local screen E2E | Complete | 1 KiB/1 MiB/100 MiB SHA matches; native repair-only late join and robust 16px test also pass |
| M14 remote desktop | SHA-verified 1 MiB/100 MiB E2E; benchmark matrix partial | [Real RDP gateway lab](benchmarks/remote-rdp-lab-20261002.md): 9/10 windowed 1 MiB cases pass across baseline and netem conditions; repair-only joining passes; 100 MiB passes in 3499.663 s at 29.3 KiB/s. Earlier [user-reported pass](benchmarks/remote-auto-user-reported.md) is retained |
| Requested WASM sender extension | Implemented; headless compatibility and browser UI checked | Worker bounded FileReaderSync reads, streaming SHA-256, existing core/Canvas, profiles/FPS/DPI/fullscreen/pause; [initial evidence](benchmarks/wasm-sender-20261002.md), [English single-file demo and offline Chromium/Firefox checks](benchmarks/web-demo-20261111.md). Browser-to-Windows physical capture and browser real RDP acceptance remain |
| Manual repair / Windows TUI extension | Implemented; core, buffer, ConPTY and local screen checks; remote acceptance pending | VFR1 independent vector, generation progress, fair targeted scheduling, default English terminal controls and `--no-tui`; [local evidence and remaining checks](benchmarks/manual-repair-tui-20261002.md) |
| Requested Linux Wayland extension | Implemented; headless, local Hyprland 1 KiB/1 MiB and Kitty TUI SHA-verified; broader acceptance pending | Native winit/softbuffer sender, ScreenCast portal/PipeWire receiver, shared TUI/manual repair, create-new output/preview/stats, interactive harness; [local Hyprland evidence](benchmarks/local-hyprland-20261016.md), [Kitty TUI evidence](benchmarks/linux-tui-20261016.md) and [remaining acceptance](linux-wayland.md) |

Before the 2026-11-11 toolchain upgrade, repository fmt, check, strict clippy and workspace tests
passed on Windows x64 with Rust 1.98.1. Release binaries and the WASM sender built successfully.
The browser adapter also passed native check and a release WASM build with the then-declared
minimum Rust 1.85.0. Five shared libraries use `no_std + alloc` and have also
passed an explicit no-default-features check. All implementation commits are local; nothing pushed.

The six PowerShell development workflows have been replaced by dependency-free Python 3.9+
scripts. Workspace and compiled WASM/native recovery checks pass on Windows and Arch Linux under
WSL; script regressions also pass on Ubuntu. The Python native harness passes local 1 KiB/1 MiB,
movement/reacquisition, robust manual-crop, empty-file and repair-only joining cases, six redirected
TUI probes and a local benchmark-launcher smoke test. See [development commands and validation](development.md).
Those migration measurements used Windows desktop capture. The subsequent Linux Wayland
extension now has [local Hyprland 1 KiB/1 MiB physical capture evidence](benchmarks/local-hyprland-20261016.md);
Linux remote acceptance remains pending.

Phase 1 has local, user-reported remote and independently executed real RDP SHA-verified evidence.
The lab records topology, executable/source hashes, actual capture rectangles, CPU/memory and
gateway rate/delay/loss settings. Baseline 1 MiB balanced runs reach 52.0 KiB/s, but the sustained
100 MiB result is below the 40–100 KB/s target. The standard 8/16 px preset matrix, remaining
large-file cases, repetitions, deliberate movement/occlusion and measured codec/video bitrate
remain incomplete. No claim of full M14 acceptance or sustained target performance is made.
Keyboard/fullscreen behavior is
implemented and compiled; exhaustive interactive GUI testing is not part of the automated suite.

An additional user-requested desktop locator is implemented: `--auto` discovers a complete
axis-aligned standard-grid frame using existing sync bands and CRC validation. It reacquires
after movement/continuous decoding failure without discarding recovery progress. Optional
`--search-rect` restricts discovery. Synthetic scaling/noise/occlusion/session-selection tests
and a native 1 MiB transfer with movement and two location locks pass. The user's remote `--auto`
run also passes SHA verification. The lab's 100 MiB case reacquired the same rectangle after a
transient interruption and retained recovery progress; deliberate remote movement remains untested.

Receiver memory remains proportional to object size; default file limit is 256 MiB. Camera
capture/perspective correction, capture API upgrades and feedback adaptation are deferred.
The receiver offers a default interactive TUI or ordinary CLI, with optional captured BMP preview;
there is no separate live image preview window. Both Windows TUI adapters and the locked workspace
check with Rust 1.85. Manual input keeps packet/FEC/visual formats unchanged, uses bounded metadata
and a one-generation sender cache, and never requires feedback. Real RDP manual-operation timings,
the 1280x720 two-window layout, Windows Terminal/traditional console comparison and exhaustive
resize/paste/error interaction acceptance remain to be recorded. Existing M14 limitations remain.

The 2026-10-16 Linux Wayland implementation preserves v0 wire/FEC semantics and the five shared
library portability boundaries. Linux workspace fmt/check/strict Clippy/tests, release build, five-core no-default-features
check and Rust 1.85 locked all-target compilation pass, separately from historical Windows measurements. Synthetic adapter recovery covers both
profiles, repair-only joining, loss/reorder/duplicates, pixel stride/bounds, location changes and
verified create-new output. After user approval, local Hyprland 0.56.2 portal/PipeWire
1 KiB/1 MiB transfers pass independent SHA-256. The 1 MiB run takes 16.283403 s
at 62.9 KiB/s including portal selection/startup. Broader portal/backend compatibility,
robust/repair-only/large-file desktop cases, movement/occlusion, fractional DPI, broader TUI
interactions, cross-platform capture and remote acceptance remain. [Kitty TUI validation](benchmarks/linux-tui-20261016.md)
passes another SHA-verified 1 MiB transfer with frozen request/apply/duplicate/copy controls,
Unicode editing, pause/resume and restored terminal modes. The completion snapshot and stale
editor notices found during testing are fixed; workspace checks now pass 51 Rust tests and
14 Python regressions. No new M14 claim is made.
