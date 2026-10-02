# Browser WASM sender

The user-requested browser sender extends the original Windows scope. It emits unchanged VFT v0
frames and uses the same standard robust/balanced profiles and deterministic sender schedule.
The existing Windows receiver can use `--auto` or an exact physical-pixel `--rect` around its Canvas.
`--sender-window` identifies the native Windows sender and does not apply to a browser.

## Build and use

From the repository root on Windows or Linux (Python 3.9+; use `python3` if needed):

```powershell
rustup target add wasm32-unknown-unknown
python scripts/build-web.py
python -m http.server 18780 --bind 127.0.0.1 --directory output/web-sender
```

Open `http://127.0.0.1:18780` in a browser supporting WebAssembly, module Workers, FileReaderSync
and Canvas 2D. Select a file, choose balanced (default) or robust, and start sending. Default FPS
is 15; default cell size is 8 physical pixels for balanced and 16 for robust. Fullscreen shows
only the frame. Space pauses/continues, F11 toggles frame fullscreen, and Esc normally exits
browser fullscreen. The browser must allow the fullscreen request from a user action.

On the receiving Windows desktop, build the native receiver and run:

```powershell
./target/release/vft-recv-win.exe --auto --output recovered.bin --preview crop.bmp
```

Keep the complete Canvas visible. The page fits integer physical cell sizes to the available
viewport, reducing the requested cell size when needed. The displayed dimensions report the
actual backing pixels; CSS dimensions compensate for devicePixelRatio and interpolation is
disabled. At less than two physical pixels per cell the page pauses and asks for a larger window.
Browser zoom, monitor DPI changes, fullscreen changes and viewport resizing trigger re-rendering.
Use the larger 8/16 px presets for remote compression where space permits.

Pause keeps the current frame and session. Hiding the document pauses without automatic resume.
Stop terminates the Worker immediately, including during the initial hash scan, and releases its
file/session resources. Restart creates a new random session and begins the systematic pass again.
The page continues repair frames until stopped; receiver completion is not signaled back.
The receiver's default 256 MiB limit still applies; larger files require an explicit receiver
limit and sufficient receiver memory. File names must meet the protocol's 255-byte UTF-8 and
path-character rules. Keep the input file unchanged until transmission ends.

The generated `output/web-sender/` contains six static files and can be served by an ordinary
HTTP server (localhost for local use) or an HTTPS static host. No runtime package installation, CDN, telemetry,
server upload endpoint, shared filesystem or connection between sender and receiver is required.
The page fetches its own JavaScript and WASM when starting a session. Starting another session
requires those assets to remain accessible; offline asset caching is not implemented. `file://`
is unsupported because of module loading/Worker restrictions. The build requires Rust, its WASM
target and Python 3.9+ for the build script; Node is only needed for development tests. Python also
provides an optional static server.

## Adapter and ABI

`apps/vft-send-web` is a `cdylib` compiled for `wasm32-unknown-unknown`. No new third-party Rust or
JavaScript dependency is introduced. A small scalar ABI avoids a generated binding toolchain:

- Imports `vft.read_at(f64 offset, u32 destination, u32 length) -> i32` and
  `vft.hash_progress(f64 done, f64 total)`.
- `vft_input_ptr()` exposes a fixed 271-byte buffer: 16 session bytes followed by up to 255
  UTF-8 filename bytes. JS writes only this buffer before calling
  `vft_init(u32 profile, f64 file_size, u32 filename_length) -> i32`.
- `vft_next() -> i32` advances one independent frame. Success is 1; failure is 0.
  `vft_frame_ptr/len` expose row-major luminance cells, not a new wire format.
- `vft_digest_ptr()` exposes 32 hash bytes. `vft_error_ptr/len` expose UTF-8 failure details.
  `vft_stat(index)` returns columns, rows, generated frames, sources, repairs and total sources
  at indices 0–5. These sender counts are not receiver rank or recovered-byte progress.

Pointers reference Rust-owned storage; JavaScript never allocates or frees Rust memory. Frame/error
pointers remain valid until the corresponding buffer is changed by init/next. The input and digest
buffers stay in the per-instance state. JS creates fresh memory views after every call because
WASM memory growth detaches older views. Imports write only their provided live exclusive slice
and must not reenter WASM. The shipped bridge catches read exceptions and returns failure; a trap
is reported by the Worker and the page terminates the failed session.

The Worker holds the File reference and uses FileReaderSync on Blob slices to implement
`ObjectSource::read_exact_at`. Rust validates metadata, scans SHA-256 with a single 64 KiB buffer,
then drops that buffer before generation encoding. The core retains one 64-symbol generation
and performs random reads on demand, including on later repair rounds. The full file is never
materialized in sender JavaScript or WASM memory. Source read failures stop the session rather
than substituting bytes.

One bounded logical frame is requested ahead of presentation. The main thread builds grayscale
RGBA cells in a small scratch Canvas, then enlarges them with nearest-neighbor rendering. It
paces draws with requestAnimationFrame, without an unbounded message queue or catch-up burst.
The FPS display counts Canvas presentation calls over active time; it does not measure monitor
refresh, unique captured frames or remote video delivery. Worker/main-thread scheduling messages
are entirely local and provide no transport ACK or adaptive feedback.

API references: [W3C File API](https://www.w3.org/TR/FileAPI/),
[FileReaderSync](https://developer.mozilla.org/en-US/docs/Web/API/FileReaderSync),
[session randomness](https://developer.mozilla.org/en-US/docs/Web/API/Crypto/getRandomValues),
[Rust WASM target](https://doc.rust-lang.org/rustc/platform-support/wasm32-unknown-unknown.html).

## Verification

```powershell
python scripts/check.py
python scripts/check-web.py # Requires Node.js 22+, plus the WASM Rust target.
```

The host adapter tests verify bounded scans, random reads, metadata/source failure handling and
both-profile SHA-verified recovery. `check-web.py` builds the actual release WASM, lints its WASM
module, checks JS syntax, and invokes it through the production JS bridge in Node. Compiled WASM
cells are rendered and sampled by the native visual library and recovered by the native receiver.
The helper independently compares SHA-256 and all bytes with the original test fixture. Empty,
1 KiB, robust/balanced 1 MiB and balanced repair-only late joining are covered with deterministic
erasures, duplicate frames and batch reordering. The scalar ABI rejects malformed arguments and
read failures, including a source becoming unreadable after hashing. All fixtures/logs stay in
ignored `output/`.

Recorded implementation evidence is in [wasm-sender-20261002.md](benchmarks/wasm-sender-20261002.md).
Node tests are headless compatibility evidence; browser UI checks and native desktop regressions
are separate. Browser-to-Windows physical screen capture and browser sender over real RDP need
their own acceptance runs. Existing native remote benchmarks are not browser performance results.
