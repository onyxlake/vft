# Local Hyprland Wayland validation — 2026-10-16

Two real local desktop transfers, 1 KiB and 1 MiB, passed on Hyprland.
Both recovered files independently match the
source SHA-256, in addition to receiver-core verification. This establishes local
Wayland software presentation, ScreenCast portal/PipeWire capture and automatic grid
discovery for this configuration. It adds no remote acceptance evidence.

## Configuration

- Source revision: `4b420ab` (`feat(linux): add Wayland sender and portal receiver`).
  Release binaries were built and checked at that revision.
- Hyprland `0.56.2`, commit `efb50993780079460b0cbed1363e2166a2de1d9f`.
- Arch packages: `xdg-desktop-portal 1.22.1-2`,
  `xdg-desktop-portal-hyprland 1.4.1-2`, `pipewire 1:1.6.9-1`.
  All three user services were active during capture.
- Available monitors reported by Hyprland: eDP-1 at 2880x1800, scale 2,
  origin `(0, 0)`, configured refresh 90.007 Hz; DP-2 at 1920x1080, scale 1,
  origin `(-1920, 0)`, configured refresh 60 Hz. Both transforms are 0.
  Configured refresh is not a measurement of displayed VFT frames per second.
- Both runs: balanced profile, requested sender 30 FPS, four physical pixels per
  cell, receiver processing ceiling 60 FPS, `--auto`, ordinary CLI (`--no-tui`).
  The harness waits one second after launching the sender before receiver startup.
- The user selects the capture source in the portal. Source label and whole-stream
  dimensions were not logged; both runs locate stream rectangle `1056 716 768 432`.
  Both saved diagnostic crops are 768x432 pixels.
- Deterministic fixtures use `VFT-python-fixture-v1`; input and output are separate
  files. The harness compares hashes, and both pairs were independently rehashed
  once more after completion.

Command:

```sh
python3 scripts/wayland-e2e.py --sizes 1024 1048576 --fps 30 --cell-size 4 --skip-build
```

## Results

| Input | Elapsed | Verified goodput | Processed captures | Valid frames | Invalid attempts | Scans / locks | Result |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 KiB | 6.517364 s | 157.119 B/s | 64 | 42 | 1 | 2 / 1 | SHA-256 match |
| 1 MiB | 16.283403 s | 64,395.383 B/s (62.9 KiB/s) | 630 | 608 | 1 | 2 / 1 | SHA-256 match |

Elapsed time starts in the receiver before the portal opens, so includes user source
selection, portal/PipeWire startup, manifest discovery, recovery, SHA verification and
output writing and portal shutdown. The one-second sender lead-in is outside this timer. Small-file times
are dominated by startup and discovery. These are one successful run per size, not
repeated throughput measurements or sustained goodput.

Valid frames include duplicate captures. Discovery pacing can skip decoding processed
captures. Invalid attempts and the difference between captures and valid frames are
not video frame loss or symbol error rates. Observed display/unique-frame FPS, CPU,
memory and video codec/bitrate were not measured; corresponding CSV fields stay blank.

Source and recovered file SHA-256:

```text
1024     96BC8927FC9F5B5D566937A6EBFAFDFD0B9D0394F08D1FB6EF10FB396D08E8AE
1048576  D8A01F8A1538C5AF49888285F88824683809D3D1CCBA7EAF343BCD82E565604B
```

Release executable SHA-256:

```text
vft-send-linux  b12ea025eb38620d5bae15dc515d002bb6e6aa8464001df8490d71f3f2ac0e55
vft-recv-linux  369e3e1d1551d792aafd138f57f7cfb98e1adacfde60115a76c63d5f67f0c7b6
```

Numeric evidence is in [local-hyprland-20261016.csv](local-hyprland-20261016.csv).
Raw fixtures, recovered files, logs, previews and per-size verified CSVs remain under
ignored `output/local-wayland-20261016-014852-dkqnql8o/` (directory timestamp is UTC).
The harness exited successfully and closed only the two sender/receiver pairs it owned.
No implementation fix was needed during these runs.

## Remaining acceptance

Subsequent [Kitty TUI checks](linux-tui-20261016.md) add a 1 MiB transfer with manual
requests, copy-view completion and restored terminal modes. The remaining cases below
continue to be separate from those terminal-driven checks.

Native robust/repair-only/empty-file tests, deliberate movement/occlusion and
reacquisition, fractional scaling or transfers across differently scaled monitors,
manual rectangle selection, portal denial/revocation, physical keyboard/fullscreen controls,
large-file runs, other portal backends, Linux/Windows cross-platform screen capture and
real remote desktop acceptance remain untested. The presence of differently scaled
monitors in the session does not establish cross-monitor behavior. Existing remote target
and sustained-throughput limitations are unchanged.
