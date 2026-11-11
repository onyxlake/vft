# Measurements

These are dated records of specific revisions and environments. Their historical toolchains,
commands and artifact sizes do not describe every later build; use the [current build guide](development.md)
and [dependency baseline](dependencies.md) for current requirements. Small CSV records and hashes
are committed here. Raw fixtures, captures and logs under ignored `output/` are local evidence,
not files included in a fresh clone.

[Native receiver output destinations](benchmarks/receiver-output-20261109.md) records
Windows 1 KiB/1 MiB SHA-verified physical capture with explicit, metadata-directory and
current-directory output modes, plus Windows/Linux headless checks on 2026-11-09.

[Local Hyprland Wayland validation](benchmarks/local-hyprland-20261016.md) records
SHA-verified 1 KiB/1 MiB native Linux desktop transfers on 2026-10-16. The 1 MiB
run takes 16.283403 s at 62.9 KiB/s including portal selection/startup; this is
local capture evidence, separate from Windows and real remote desktop measurements.
[Linux Kitty TUI validation](benchmarks/linux-tui-20261016.md) records an additional
SHA-verified 1 MiB transfer, repair-request controls, terminal restoration and display fixes.

Browser sender evidence is recorded separately in
[WASM sender validation](benchmarks/wasm-sender-20261002.md): compiled WASM/native SHA-verified
recovery, bounded-read/memory observations and browser UI checks. Later
[single-file demo checks](benchmarks/web-demo-20261111.md) cover offline Chromium/Firefox operation.
These are not remote goodput results.

## Local Windows screen capture

Measured 2026-09-30 on Windows x64 (`10.0.26200.0`), Rust 1.98.1, release build, with separate
native sender/receiver processes and actual screen capture. These are **local desktop** results.
The local measurements below remain separate from the later
[user-reported remote `--auto` result](benchmarks/remote-auto-user-reported.md) and the
[controlled real RDP lab results](benchmarks/remote-rdp-lab-20261002.md).

| Input | Profile / requested FPS / cell | Verified time | Verified goodput | Result |
| --- | --- | --- | --- | --- |
| 1 KiB | Balanced / 15 / 8 px | 2.374 s | 431 B/s | SHA-256 match |
| 1 MiB | Balanced / 60 / 4 px | 16.050 s | 65,332 B/s (63.8 KiB/s) | SHA-256 match |
| 100 MiB | Balanced / 60 / 4 px / topmost | 1171.705 s | 89,491 B/s (87.4 KiB/s) | SHA-256 match |
| 1 MiB, join after 15 s | Balanced / 60 / 4 px / topmost | 11.875 s after join | 88,298 B/s (86.2 KiB/s) | Repair-only, SHA-256 match |
| 1 KiB | Robust / 10 / 16 px / topmost | 3.427 s | 299 B/s | SHA-256 match |

Small-file timings are dominated by manifest discovery and repair scheduling. The 1 MiB run had
301 rejected captures out of 962, with 225 accepted symbols for 215 sources. A short local result
must not be extrapolated into sustained remote throughput. Four-pixel cells were selected for a
smaller local test window; remote presets remain 8/16 pixels.

The 100 MiB run had 42 rejected captures out of 70,227, 21,470 independent symbols and 417
dependent equations. It completed all 336 generations and independently verified the output hash.
Observed adjacent unique-frame rate was 36.36/s despite a requested 60 FPS. Sender CPU time was
107.125 seconds and peak working set 17,887,232 bytes. Recovery latency includes the full first
source pass and round-robin 64-repair bursts; later generations wait their turn even if only a few
symbols are missing. This is a baseline measurement, not an optimized scheduling claim.

The native late-join run captured **zero source frames** and 691 valid repair frames, including
duplicate captures. All four generations were reconstructed from repairs alone. Its reported time
starts at receiver launch after a 15-second sender lead-in; sender-start time was about 26.9 seconds.

Numeric records and hashes are preserved in [local-windows.csv](benchmarks/local-windows.csv).
Blank cells mean that counter was not collected in that run. Source/repair frame counters include
duplicates and packets observed before manifest discovery; they are distinct from accepted FEC
equations. New local runs capture CPU/memory data for both processes.

Exact successful source hashes:

```text
1 KiB  07FFCC47372D3EF3E3ACC2E59BA6D0129A2979CA501674E5021E105B9FDEE1F7
1 MiB  20AA6FF299998FDD317DE437354EEA93D81B34EC37CF1004B8C3851642CE9CFD
100 MiB 1AC26D6D7497A12D8E86EFF233C8E3604332C967F9DF585C55335134AF4CA552
```

Raw local runs are generated under ignored `output/local-*`: original fixture, recovered file,
SHA-verified result CSV, sender/receiver logs, capture rectangle and diagnostic BMP. Fixtures use
`.NET Random(20260930)` filled in 64 KiB chunks in the historical PowerShell harness. The Python
replacement uses different deterministic fixtures; see [development scripts](development.md).
Retain original hashes when reproducing on another .NET runtime because fixture generation
is test tooling, not a protocol contract.

Observed environmental failures were retained, not counted as successful transfers:

- Sandboxed desktop capture returned access denied; successful native tests ran on the user's
  interactive desktop.
- An early launcher hid the sender window through Win32 startup flags; showing the visual window
  explicitly fixed the test launch.
- The first 100 MiB attempt was interrupted after another application covered the sender window,
  yielding no further valid packets. A fresh attempt uses `--topmost` to keep the channel visible.

The local test script closes only the processes it starts and refuses to overwrite recovered output.
It adds CPU/memory counters to new runs; some initial runs predate receiver CPU instrumentation.
`python scripts/remote-benchmark.py` records receiver CPU and memory plus separately supplied transport
configuration. It does not invent unknown video bitrate, codec, sender CPU, or frame-loss numbers.

## Automatic location and capture-mode regression

Additional local release runs validate the visual locator, with independent input/output
SHA-256 comparisons. These are functionality checks, not repeated throughput benchmarks:

| Capture mode | Input / profile | Verified time | Location scans / locks | Result |
| --- | --- | --- | --- | --- |
| `--auto`, sender moved after 3 s | 1 MiB / balanced / 60 FPS / 4 px | 9.687 s | 2 / 2 | Reacquired, SHA match |
| `--auto` | 1 KiB / robust / 10 FPS / 16 px | 0.153 s | 1 / 1 | Located, SHA match |
| `--sender-window` | 1 KiB / balanced / 60 FPS / 4 px | 0.739 s | 0 / 0 | SHA match |
| `--sender-window` | 1 MiB / balanced / 60 FPS / 4 px | 9.207 s | 0 / 0 | SHA match |
| `--rect` | 1 MiB / balanced / 60 FPS / 4 px | 9.634 s | 0 / 0 | SHA match |

The movement run first located `49 78 768 432`, then `234 188 768 432`; 47 captures were rejected
while the old crop was invalid. Recovery progress survived reacquisition. The native script uses
the sender rectangle file only for test readiness/recording; the `--auto` receiver receives no
sender coordinates or HWND. Robust auto discovery also succeeds at the original 1536 × 864 size.

Records are in [capture-modes-windows.csv](benchmarks/capture-modes-windows.csv). `capture_region`
is the sender's final recorded client region, so the movement run's first location is in its log.
Search scans with no detected frame are separate from capture rejection counters. Small runs
can finish between the one-second process-memory polls; their unknown receiver peak memory is
blank. Small-file timing depends on the packet visible at startup and is not sustained goodput.

The synthetic suite additionally covers screen-edge placement, both profiles, two-pixel cells,
fractional independent X/Y scaling, blur/noise/contrast, occlusion, CRC corruption, false patterns,
and selecting the existing session among multiple senders. A subsequent user-provided remote
completion log is recorded separately below.

## User-reported remote desktop result

On 2026-09-30 the user reported a successful `--auto` remote desktop test and supplied the
receiver's `COMPLETE sha256=verified` line. It recovered 617,787 bytes in 22.077 s at 27.3 KiB/s.
The log reports 654 received packets, 436 duplicate symbols, 127 independent symbols, two
completed generations and one invalid visual capture. See the
[exact log and limitations](benchmarks/remote-auto-user-reported.md).

This supplies remote functional E2E evidence. The transport product/codec, profile, FPS, cell
size, display settings and repeated trials are unknown. Goodput is approximately 28.0 decimal
KB/s, below the remote 40–100 KB/s target. It does not establish remote movement/reacquisition,
repair-only joining or the complete benchmark matrix.

Run the native fixtures with:

```powershell
python scripts/local-e2e.py --sizes 1024 1048576 104857600 --fps 60 --cell-size 4
# Auto discovery followed by moving only the test-owned sender window:
python scripts/local-e2e.py --sizes 1048576 --fps 60 --cell-size 4 --auto-locate --move-sender
# Manual rectangle regression, obtained by the test harness from the sender:
python scripts/local-e2e.py --sizes 1048576 --fps 60 --cell-size 4 --manual-rect
```

For a repair-only native join, choose `--join-delay-seconds` greater than the source transmission time.
The simulator already tests joining after all sources have passed. Follow [remote validation](remote-validation.md)
for the 5/10/15/30 FPS × profile/cell-size matrix. The reported pass does not identify a matrix
configuration; full matrix coverage remains unrecorded.
