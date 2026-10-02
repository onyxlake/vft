# Manual repair and Windows TUI local validation

Date: 2026-10-02, Asia/Shanghai. This records implementation and local evidence, not M14
acceptance or a real remote manual-repair speedup. The accepted scope is
[manual-repair-tui-plan.md](../manual-repair-tui-plan.md).

## Automated evidence

The workspace passes formatting, check, strict all-target clippy and tests. The locked Windows
workspace checks with Rust 1.85.0; shared libraries check without default features. Release builds
and `check-web.ps1` pass, including native decoding of WASM source/repair-only transfers. The
terminal backend is pinned to Ratatui 0.29.0 / Crossterm 0.28.1; the lockfile uses instability
0.3.7 because the initially resolved 0.3.14 required Rust 1.88. Revalidate MSRV when updating it.

Coverage includes independently computed VFR1 bytes, tag/CRC, hexadecimal normalization,
truncation, duplicate/range/deficit/length errors, last/unseen/completed groups, overflow,
metadata refusal, retry after source failure, recent-ID conflict/no-op, cancellation, lazy cursor
initialization after automatic repairs, ordinary-source/repair fairness and manifest priority.
Terminal model/buffer tests cover safe Unicode editing, controls, bounded frozen request pages,
busy/disconnected commands, mode selection and key/status visibility at 100x28, 80x24 and 40x10.
Receiver pre-hash notification does not expose recovered data and is not repeated after completion.

`manual_repair.rs` uses a 10,240-byte fixture, ten 64-symbol generations, sixteen-byte symbols,
seed-37 sequence-based 18% loss plus a 12/97-frame burst gap, duplicates and reversed arrival
batches. It deliberately omits initial source packets for groups 1 and 10. Both profiles verify
the same SHA. Baseline completion is frame 1954; manual completion is frame 1185. From at least
80% completed generations, remaining frame counts are 730 versus 54. These are deterministic
headless frame counts, with instantaneous request application and no human input time; they are
not elapsed-time or goodput benchmarks. Manual repair-only late joining also passes.

## Local screen and real terminal controls

`output/local-20261002-135514-f6eb61c1/` contains the CLI regression with explicit `--no-tui`:
balanced, requested 60 FPS, four-pixel cells, topmost, sender-client capture at `49 78 768 432`.
The independent PowerShell SHA comparisons pass:

| Bytes | Receiver seconds | Verified bytes/s | Adjacent unique valid frames/s |
| --- | --- | --- | --- |
| 1024 | 0.555191 | 1844.409 | 39.626 |
| 1048576 | 7.119904 | 147273.900 | 39.888 |

These counters measure adjacent unequal decoded packets, not requested presentation FPS or
video loss. CPU/memory samples and raw counters remain in the generated results CSV.

An additional balanced 1 MiB `--auto -MoveSender` regression passes in 9.390443 seconds with
111664.167 verified bytes/s, 36.526 adjacent unique valid frames/s and two location locks. The
final physical rectangle is `234 188 768 432`; diagnostics are under
`output/local-20261002-140705-3be5da7f/`. Robust four-pixel CLI regression also passes: 1 KiB in
0.719711 seconds and 1 MiB in 97.340767 seconds (10772.218 verified bytes/s, 39.963 adjacent
unique valid frames/s), at `49 78 384 216`, under `output/local-20261002-141244-79d2e60d/`.
The slower robust result is not compared with manual operation or extrapolated to remote performance.

`check-tui-cli.ps1 -SkipBuild` passes all six redirected-mode probes, with no application terminal
escapes in stdout/stderr and English errors for explicit TUI and conflicting flags. Evidence is
in `output/tui-cli-20261002-141131-4648b1ef/`. A subsequent ConPTY sender test confirms Space
sets Paused and freezes its presentation counter, a second Space resumes it, and Ctrl+C exits
successfully with terminal restoration. Full physical-keyboard coverage remains separate.
An explicit ConPTY receiver TUI with no local sender keeps a Failed result visible until Q,
then restores the terminal and returns exit code 1. The probe exposed FindWindow's misleading
localized zero-error diagnostic; the final adapter now adds a specific English missing-sender
message, retaining the Windows code only as diagnostic detail.

An 80x28 interactive ConPTY session launched both applications without `--tui`. Both chose their
English TUI. The Win32 sender retained its separate visible window. The input fixture was
`bytes(range(256)) * 4096`, balanced, requested 60 FPS, four-pixel cells, topmost; receiver used
`--sender-window`, seven requested captures/s and joined after sources. Through actual terminal
input, the receiver selected group 1 and generated the frozen code
`VFR1/DDBF01555850AA81/9D8C9E5394A123B8/1:53/F827E9AD`. The sender editor also previewed simple
`1:3` (nine repairs) before cancelling that edit, then previewed and applied the full code
(84 new repairs). The sender showed Request applied, manual packet progress, and Manual repair
batch sent. Editing did not pause the visual frame counter. The frozen code remained visible
while receiver ranks advanced; Verifying SHA-256 and Complete were observed. Both terminals
emitted paste-disable / alternate-screen-leave / cursor-show on Q and exited successfully.

The receiver independently produced SHA-256
`FBBAB289F7F94B25736C58BE46A994C441FD02552CC6022352E3D86D2FAB7C83`, matching the input with
PowerShell Get-FileHash. Its stats report 44.987703 seconds, 315 captures / valid / adjacent
unique frames, 7.002 adjacent unique frames/s, 216 accepted symbols, one dependent symbol,
zero CRC failures and 23308.058 verified bytes/s. This elapsed interval includes the terminal
operation during capture. Input/preview/apply timing and request-to-first-packet timing were
not separately instrumented; neither CPU/memory nor an automatic baseline was measured for this
TUI case. It establishes a local functional path, not causality or a measured manual speedup.

Evidence lives in `output/manual-tui-1790949399257783900/`. An earlier two-captures/s attempt was
stopped after 73.051900 seconds: 146 valid repair frames arrived but no manifest was captured,
so no session or verified output existed and verified goodput was zero. `stats.csv` preserves
this diagnostic; `stats-7fps.csv` belongs to the successful retry. Periodic capture can miss
periodic manifests; capture rates below the repeated-manifest cadence should be avoided.

The binaries used for the above live run, before final compact-help/session-title adjustments:
sender SHA `0030790BBDB6CF6E11F70C4AFACBE26D5D915AE5E24BD530874B391685E9A4A4`, receiver SHA
`01AE56795E357BDB61FB116D1AF257626C59F6C5B5603A267440A14E371B04CE`. Generated fixtures and
capture/stat files are ignored; only this small measurement record is committed.

A subsequent final-build ConPTY check exercised P's main-screen copy view. It printed the full
frozen VFR1 code on one logical line, left capture running, and reported verified completion
without rewriting that code. Tab returned to the alternate-screen TUI with the same request;
Q restored both terminals. The independent input/output hashes above matched again. Evidence
is `copy-recovered.bin` and `copy-stats.csv` in the same ignored folder: 44.558928 seconds,
312 captures / valid / adjacent unique frames, 7.002 adjacent unique frames/s, 225 accepted
symbols, ten dependent symbols, zero CRC failures and 23532.344 verified bytes/s. This copy-view
check generated a request but did not apply it at the sender; clipboard selection itself was
not automated. Regression tests also cover long codes and fresh request IDs generated within
the same timestamp, preventing stale request page indices.

## Remaining acceptance

Real Windows Terminal and traditional-console comparison, bracketed-paste/resize coverage,
all lifecycle/error paths under physical keyboard input, and the complete two-window layout at
1280x720/single-screen remote desktop remain. ConPTY and buffer tests do not establish these.
No real RDP 1 MiB / 100 MiB comparison with three repetitions, human transcription time,
request latency, rank-rate history, per-request read costs or CPU/memory was measured here.
Those checks remain in P6. Browser manual-input UI is outside this first implementation;
automatic WASM compatibility is preserved. Existing remote throughput limitations remain.
