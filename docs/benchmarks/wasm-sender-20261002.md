# WASM sender implementation validation — 2026-10-02

This records the requested browser adapter extension, independently of M14 native RDP acceptance.
No browser remote goodput or browser-to-Windows physical screen transfer is claimed here.

## Build and compatibility

- Windows x64, Rust 1.98.1; release `vft_send_web.wasm`: 64,896 bytes.
- Artifact SHA-256: `EB619A10770E1A51FB46EDEF606C4986B832B19B21100340E5528EF94592AAA0`.
- Rust 1.85.0 (declared minimum): native adapter check and release `wasm32-unknown-unknown` build pass.
- No new external Rust/npm dependency. Five shared libraries also check with
  `--no-default-features --target wasm32-unknown-unknown`.
- `scripts/check.ps1`: format/check/strict all-target clippy/27 tests pass.
- `scripts/check-web.ps1`: release WASM build, WASM-target strict clippy, JS syntax,
  production JS bridge plus native visual/receiver recovery pass.

## Actual compiled WASM → native receiver

`scripts/web-e2e.mjs` uses deterministic fixtures and session `[7; 16]`. It drops one frame in seven,
reverses nine-frame batches and duplicates a frame in each batch. Late joining discards all frames
before the first repair symbol, so discovery occurs from a later repeated manifest. The native
helper renders the exported logical cells at four pixels/cell, samples the pixels, checks packet
CRC through the receiver, verifies file SHA-256, and independently compares all recovered bytes
and SHA-256 with the original fixture.

| Profile | File bytes | Join | SHA/byte equality | Receiver packets to completion | WASM linear memory after test | Maximum single file read |
| --- | ---: | --- | --- | ---: | ---: | ---: |
| Robust | 0 | Start | Pass | 1 | 1,310,720 bytes | 0 |
| Balanced | 1,024 | Start | Pass | 10 | 1,310,720 bytes | 1,024 bytes |
| Robust | 1,048,576 | Start | Pass | 3,847 | 1,310,720 bytes | 65,536 bytes |
| Balanced | 1,048,576 | Start | Pass | 404 | 1,507,328 bytes | 65,536 bytes |
| Balanced | 1,048,576 | Repair only | Pass | 388 | 1,507,328 bytes | 65,536 bytes |

These memory figures count WASM linear memory, not browser/Node process memory, File backing
storage or Canvas/GPU allocation. This is a headless correctness test, not a timing/goodput
benchmark. Read failure during hashing and after a successful hash, invalid scalar arguments,
and calls before initialization are also tested. Source failure does not advance sender progress.

Successful raw runs: ignored `output/web-e2e-1790925107014/` and
`output/web-e2e-1790926557471/`; each contains fixtures, delivered frames and `results.json`.

## Browser UI

The Codex in-app Chromium browser loaded the built static page from loopback port 18780. Tests
selected real local 1 KiB/1 MiB files through the browser file chooser and used the production
module Worker, FileReaderSync, Rust hashing and Canvas display.

- Balanced and robust start successfully. Switching to robust selects the 16 px default.
- Browser SHA-256 matches an independent Node SHA-256: 1 KiB
  `55d4e7493d6c40f3bcead8ee5475594d242c7db59e45a0973ebcdf0737e5b05f`;
  1 MiB `82880096e611d2f6ca990ca5d9b66ba63cfa402911755ac13fc66998315b7b5e`.
- Pause retains the frame, digest and session; the displayed count stayed at 911 across checks,
  then continued after Resume. Stop clears the Canvas and enables file/configuration controls;
  starting again works with the same selected file in the other profile.
- At a 1280×720 CSS viewport, DPR 1.25, balanced fitted to a 768×432 backing Canvas
  (4 physical pixels/cell). Its DOM bounds remained inside the viewport.
- The fullscreen button entered frame-only fullscreen and selected 1536×864 backing pixels.
  Page reload returned to the normal UI. Automated Esc/F11 exit actions were inconclusive in
  the embedded browser; native browser keyboard behavior still needs interactive confirmation.
- One-second scheduled Canvas FPS counters showed 15.0 at target 15; 57.9 (balanced) and 58.0
  (robust) at target 60. These are UI presentation counters, not unique displayed/captured or
  remotely delivered frame rates. Browser version, display refresh, CPU, whole-process memory
  and codec settings were not measured.
- Saved proof screenshot: ignored `output/web-sender/browser-proof.png` (robust, paused).
  Tab-hidden behavior and the too-small-viewport pause path are implemented but were not
  conclusively exercised by the embedded browser's independent-tab model.

Port 8080 was unavailable in this host environment; switching to 18780 succeeded. No global
network or browser security setting was changed.

## Existing native desktop regression

`scripts/local-e2e.ps1 -Sizes 1024,1048576 -Fps 60 -CellSize 4 -AutoLocate -SkipBuild`
passes against the release native Windows applications. SHA-256 matches independently for both
files. Raw evidence: ignored `output/local-20261002-072444-0fb92d76/`.
This protects the existing native path and does not establish browser screen-capture acceptance.

Still needed: browser Canvas → Windows physical capture with independent recovered-file SHA,
browser sender in the real RDP/netem lab, additional browser engines, deliberate resize/DPI
movement/occlusion and sustained remote performance. Original native M14 evidence remains in
[remote-rdp-lab-20261002.md](remote-rdp-lab-20261002.md).
