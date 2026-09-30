# Real remote desktop validation

M14 has not been performed: no remote desktop session or remote sender was supplied. Local desktop
and synthetic transport results must not be reported as RDP/video results. The 40–100 KB/s remote
goodput target remains an unverified target.

## Setup

Build `cargo build --release --workspace` on Windows x64. Copy the sender executable and a known
fixture to the remote side through an allowed provisioning route; this is setup, not the transfer
channel being measured. Keep the remote desktop unlocked and visible. Hash the fixture on the
sender before testing, and record file length, screen resolution, scaling, transport settings,
codec and independently measured video bitrate. Unknown values must stay unknown.

On the sender:

```powershell
vft-send-win.exe fixture.bin --profile balanced --fps 15 --cell-size 8 --topmost
```

On the receiver, determine the physical-pixel rectangle containing only the full visual frame
(exclude the title bar/borders). This manual crop is the intended Phase 1 capture input:

```powershell
./scripts/remote-benchmark.ps1 -X 50 -Y 80 -Width 1536 -Height 864 `
    -Profile balanced -SenderFps 15 -CellSize 8 -Transport RDP -Codec unknown `
    -ExpectedSha256 THE_HASH_FROM_THE_SENDER
```

Coordinates above are examples, not a detected rectangle. The script creates unique output files,
configuration JSON, a crop preview, receiver logs, receiver CPU/memory statistics, and verified
payload goodput. It never communicates with the sender or marks an unsuccessful run as verified.

For interactive use without measuring a manual crop, the receiver now supports:

```powershell
vft-recv-win.exe --auto --output recovered.bin --preview crop.bmp --stats stats.csv
# Restrict discovery to the local area displaying the remote desktop if needed:
vft-recv-win.exe --auto --search-rect 0 0 1920 1080 --output recovered.bin
```

The locator reads screen pixels, so it also works with a remote sender displayed locally; it does
not require a local sender HWND. Keep the entire axis-aligned frame visible. Discovery and
reacquisition have local/synthetic validation only; their real remote codec performance remains
unmeasured. The benchmark runner above still uses an explicitly recorded manual rectangle.

## Matrix

Run each combination with 1 MiB and 100 MiB fixtures, preferably at least three repetitions:

| Profile | Cell pixels | Sender FPS |
| --- | --- | --- |
| Robust (binary) | 16 | 5, 10, 15, 30 |
| Robust (binary) | 8 | 5, 10, 15, 30 |
| Balanced (four-level) | 16 | 5, 10, 15, 30 |
| Balanced (four-level) | 8 | 5, 10, 15, 30 |

Resize the sender window or use fullscreen when its native dimensions exceed the remote screen.
Use the actual displayed frame rectangle on the receiver. Changing display size changes physical
cell size; record this instead of assuming the CLI cell size survived scaling.

Also test midstream joining after all source frames have passed, repeated capture frames, deliberate
window occlusion, reduced bandwidth, and both endpoint DPI settings. Remove occlusion before
waiting for repairs. Verify SHA-256 independently on the output. ESC stops the sender after a run.

## Metrics and interpretation

Verified payload goodput is original file length divided by receiver elapsed time, including
manifest wait, capture, decoding, repairs, SHA verification and file output. Failed runs report zero
verified goodput. A sender start-to-completion measurement should be recorded separately if needed.

Receiver CSV includes captures, valid/unique frames, rejected frames, CRC failures, symbols,
duplicates, dependencies, recovered bytes and verification. `unique_frames` counts adjacent changes
among valid packets, including repeated manifests. `packet_error_rate` is the fraction of captures
rejected by visual decoding, so it includes sync/crop failures as well as CRC errors. It is not
video frame loss. `fec_overhead` is `(accepted valid symbols / source symbol count) - 1`; repairs for
already completed generations are ignored, so also retain sender progress if total channel overhead
is needed. Symbol error rate is not directly observable for CRC-rejected packets.

`source_frames` and `repair_frames` count all CRC-valid captures of each packet type, including
duplicates and arrivals before discovery. A repair-only late-join run must show zero source frames.

`location_scans` and `location_locks` describe automatic discovery/reacquisition. Full search
screenshots with no detected frame are not included in frame capture/rejection counters, but
their elapsed time is included in verified goodput. Both counters are zero in manual capture mode.

Record actual unique-frame rate, application CPU seconds, wall duration and peak memory. The script
does not measure video bitrate or the remote sender's CPU. Collect those with endpoint/transport
instrumentation and attach the evidence to the run. Do not derive missing frames from the requested
FPS alone: Windows timers, composition and the codec can repeat or drop frames.

Document successful and failed runs. M14 and Phase 1 Done require real remote SHA-verified E2E,
benchmark results, and passing repository checks; adding this procedure does not satisfy them.
