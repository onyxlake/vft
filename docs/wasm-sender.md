# Browser WASM sender

The browser sender emits VFT v0 frames using the standard robust/balanced profiles and
deterministic sender schedule.
The existing Windows receiver can use `--auto` or an exact physical-pixel `--rect` around its Canvas.
`--sender-window` identifies the native Windows sender and does not apply to a browser.

## Build and use

From the repository root on Windows or Linux (Python 3.9+; use `python3` if needed):

```powershell
rustup target add wasm32-unknown-unknown
python scripts/build-web.py
```

Open the generated `output/web-sender/index.html` directly in a browser (`file://`). It is a
self-contained file: copy just this HTML file to another machine, with no server or sibling
assets required. The page has a plain English demo UI with native form controls. The browser
must support WebAssembly, classic Blob Workers, FileReaderSync and Canvas 2D.
Select a file, choose balanced (default) or robust, and start sending. Default FPS
is 15; default cell size is 8 physical pixels for balanced and 16 for robust. Fullscreen shows
only the frame. Space pauses/resumes, F11 toggles frame fullscreen, and Esc normally exits
browser fullscreen. The browser must allow the fullscreen request from a user action.

On the receiving Windows desktop, build the native receiver and run:

```powershell
.\target\release\vft-recv-win.exe --auto --output recovered.bin --preview crop.bmp
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
The receiver's default 256 MiB limit applies. Larger files require `--max-file-size BYTES` and
sufficient receiver memory; native receivers also retain a 1,000,000-source-symbol limit that
this option does not change. That caps standard robust transfers at 531,000,000 bytes and balanced
transfers at 4,884,000,000 bytes, even with a higher byte limit. See [resource limits](cli.md#resource-limits).
File names must meet the protocol's 255-byte UTF-8 and path-character rules. Keep the input file
unchanged until transmission ends.

The distribution contains only `index.html`. CSS and the main script are inline; the bridge and
Worker are bundled into an escaped JSON script block, and WASM is embedded as base64. Starting
a session creates a classic Worker from a Blob URL and sends it the selected File and embedded
WASM. The Worker decodes and instantiates WASM from bytes, without `fetch`, module imports or
`importScripts`. This avoids cross-origin local-file asset requests rather than disabling CORS.
Stop/error cleanup terminates the Worker and revokes its Blob URL; restart works offline.

The same HTML also includes the VFT MIT license, full MIT notices for the locked WASM Cargo
dependencies (including build dependencies), and the selected Rust toolchain's copyright and
license texts. Open **Licenses and third-party notices** in the footer to read them offline.
Toolchain notices are retained conservatively and may cover components not linked into WASM.
The build resolves locked Cargo metadata, downloading uncached package sources when needed,
then reads dependency notices from those sources and toolchain notices from
`share/doc/rust` in the selected `rustc` sysroot. Missing notices or an unsupported dependency
license stop the build; review license changes before extending `scripts/web_licenses.py`.
Keep this embedded section when redistributing the single HTML file.

HTTP/HTTPS hosting is still supported, for example with
`python -m http.server 18780 --bind 127.0.0.1 --directory output/web-sender`.
If a host sets a Content Security Policy, it must permit the inline scripts/styles, Blob Workers
and WebAssembly compilation. Browser or organization policies can still restrict these APIs.
No runtime package installation, CDN, telemetry, upload endpoint, shared filesystem or connection
between sender and receiver is required. The build needs Rust, its WASM target and Python 3.9+;
Node is only needed for development tests. Source files stay separate for editing; rebuild after
changes rather than opening the source HTML template. Rebuilds remove only the obsolete generated
JS/CSS/WASM assets from the previous multi-file distribution and preserve unrelated output files.

## Adapter and ABI

`apps/vft-send-web` is a `cdylib` compiled for `wasm32-unknown-unknown`. It depends on the shared
Rust cores and uses no third-party JavaScript packages. A small scalar ABI avoids a generated binding toolchain:

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
[embedded Workers](https://developer.mozilla.org/en-US/docs/Web/API/Web_Workers_API/Using_web_workers#embedded_workers),
[FileReaderSync](https://developer.mozilla.org/en-US/docs/Web/API/FileReaderSync),
[session randomness](https://developer.mozilla.org/en-US/docs/Web/API/Crypto/getRandomValues),
[Rust WASM target](https://doc.rust-lang.org/rustc/platform-support/wasm32-unknown-unknown.html).

Build and test commands, including packaged WASM/native recovery, are in the
[build and test guide](development.md).
