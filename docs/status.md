# Implementation status

Updated 2026-09-30 (Asia/Shanghai). The original scope is preserved in [plan.md](plan.md).

| Milestone | Status | Evidence |
| --- | --- | --- |
| M0 workspace | Complete | Eight workspace members, format/lint setup and local Git checkpoints |
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
| M14 remote desktop | SHA-verified E2E pass; benchmark matrix partial | User tested `--auto` remotely: 617,787 bytes, 22.077 s, 27.3 KiB/s; [completion log](benchmarks/remote-auto-user-reported.md). Profile/FPS/configuration and broader matrix remain unreported |

Repository fmt, check, strict clippy and all 25 tests pass on Windows x64 with Rust 1.98.1.
Release binaries build successfully. Five shared libraries use `no_std + alloc` and have also
passed an explicit no-default-features check. All implementation commits are local; nothing pushed.

Phase 1 now has both local and user-reported real remote SHA-verified E2E evidence, plus a first
remote goodput record. M14's full FPS/profile benchmark matrix and reproducible configuration
remain incomplete. The reported remote run is below the 40–100 KB/s target; no claim of meeting
that target is made. Keyboard/fullscreen behavior is
implemented and compiled; exhaustive interactive GUI testing is not part of the automated suite.

An additional user-requested desktop locator is implemented: `--auto` discovers a complete
axis-aligned standard-grid frame using existing sync bands and CRC validation. It reacquires
after movement/continuous decoding failure without discarding recovery progress. Optional
`--search-rect` restricts discovery. Synthetic scaling/noise/occlusion/session-selection tests
and a native 1 MiB transfer with movement and two location locks pass. The user's remote `--auto`
run also passes SHA verification; remote movement/reacquisition is not yet reported.

Receiver memory remains proportional to object size; default file limit is 256 MiB. Camera
capture/perspective correction, capture API upgrades and feedback adaptation are deferred.
The first receiver is a CLI with optional captured BMP preview, not a separate live preview window.
