# Architecture

```text
protocol ← fec
    ↑       ↑
  visual ← sender
    ↑       ↑
    └── Windows sender (file source, SHA-256 scan, Win32/DIB)
    └── Linux Wayland sender (file source, winit/softbuffer)
    └── WASM sender (Worker FileReaderSync source, SHA-256 scan, Canvas)

protocol + fec → receiver
visual + receiver → Windows receiver (physical screen rectangle, GDI capture, file output)
visual + receiver → Linux receiver (ScreenCast portal, PipeWire, verified file output)
sender + visual + testkit + receiver → headless integration tests
```

`vft-protocol`, `vft-fec`, `vft-visual`, `vft-sender`, and `vft-receiver` are `no_std + alloc`.
They use no filesystem, Windows, UI or capture APIs. `vft-testkit` uses std for test tooling.
The only external shared dependency is SHA-256 (`sha2` with default features disabled).
Windows APIs are target-specific dependencies of the two applications.

`apps/vft-tui` supplies common English control models, Unicode input editing, bounded logs and a
Windows/Linux Ratatui 0.29 / Crossterm 0.28 backend. Cargo.lock pins its transitive dependencies
and the locked workspace is checked with Rust 1.85. Native apps choose TUI once at startup when
stdin and stdout are interactive; `--no-tui` preserves script behavior and explicit `--tui`
requires both terminals. Help and argument errors remain ordinary text.

The sender's Win32 thread owns SenderSession, HWND and GDI. A terminal thread sends small commands
through a 32-slot queue and posts a private wake message; changes occur at guarded message
boundaries. Stop has a separate atomic signal, including while paused. The receiver creates and
destroys Capture and ReceiverSession on its capture worker; the main thread owns its terminal.
Progress replaces a latest snapshot, with 5 Hz rendering, six-row incomplete-group pages and a
200-line log ring. Request text is frozen at a capture boundary and paginated into 256-target
codes with OS-random IDs. No pixels or recovered file copies cross into the terminal.

Receiver verification notifies the UI immediately before hashing; Complete is published only
after SHA verification and create-new output succeed. TUI results remain visible until Quit,
while CLI completion still exits. RAII restores raw mode, alternate screen, cursor and bracketed
paste after normal exits, errors and unwind; workers are stopped and joined before final summaries.
Forced process termination cannot restore a terminal. The independent visual window remains the
transport and must be kept clear of the control terminal. Manual text does not create an automated
feedback channel or imply receiver completion at the sender.

Sender input implements `ObjectSource::read_exact_at`. The Windows adapter keeps an open file,
hashes it in 64 KiB chunks, and seeks on demand. Encoder caching retains only one 64-symbol
generation (about 305 KiB for balanced), plus frame buffers. The browser adapter supplies the same
random-access trait from a dedicated Worker using FileReaderSync on bounded File/Blob slices.
It hashes in 64 KiB chunks, transfers one logical frame at a time to the main thread, and renders
it through nearest-neighbor Canvas scaling with physical-pixel/DPI sizing. Pause/hidden-page handling
keeps the current session; stop terminates the Worker. Its small scalar WASM ABI requires no new
binding dependencies. See [wasm-sender.md](wasm-sender.md) for build, lifecycle and ABI contracts.
The file must remain unchanged for
the transfer. No ACK, completion feedback or shared filesystem is required by either core.

Receiver FEC uses one lazily allocated decoder per incomplete generation. Completed generation
bytes stay in memory, allowing ordered SHA-256 validation and verified output chunks without a
second full-file copy. This first receiver is memory-backed, with a 256 MiB default file limit;
disk-backed recovery for larger objects is future work. `reconstruct()` explicitly makes a full
copy; `verified_chunks()` avoids it. Sender streaming support does not imply bounded receiver
memory. Gaussian elimination and symbol generation have bounded 64-symbol algebraic complexity.

The Windows sender paints a software BGRA framebuffer in one `StretchDIBits` call. Its message
callback holds a stable boxed `RefCell` for state, catches panic at the FFI boundary, and guards
against synchronous Win32 message reentry. ESC closes, SPACE pauses, F11 switches fullscreen.
The optional `--topmost` mode prevents the visual channel from being covered during benchmarks.
Requested FPS is a timer target, not a guarantee of unique screen frames.

The Windows receiver uses `GetDC`, a reusable compatible DC and top-down `CreateDIBSection`,
`BitBlt`, then `GdiFlush` before reading the pixel buffer. GDI resources are restored/released by
RAII. Per-monitor-v2 DPI awareness keeps capture coordinates in physical pixels. It can follow
the local sender's client region, capture a manually supplied rectangle, or visually locate a
frame using `--auto`. Automatic discovery captures the virtual desktop (or `--search-rect`), runs
the platform-neutral locator, then switches to the CRC-validated crop and releases the large
discovery DIB. Continuous decoding failure for 750 ms triggers reacquisition while keeping the
receiver session and recovered generations. Full-desktop searches are paced to a 500 ms minimum
iteration interval. `--preview` saves one BMP of the first captured/located frame for crop
diagnostics. Output uses create-new semantics after core SHA verification.

Both native receivers include the std-only `apps/receiver_output.rs` module for destination
validation, outside the portable cores. `--output-dir` defaults to the current working
directory; without an explicit `--output`, the accepted manifest supplies the filename.
The directory must exist. Explicit output paths take precedence and retain their supplied
meaning. Resolution rejects unsafe metadata names, existing destinations and aliases of
preview/statistics paths, and is reported before recovered output is written. Final file
creation still uses create-new after SHA verification to protect against later collisions.

Linux adapter helpers live in `apps/vft-linux`, outside the portable cores. The native
sender uses a Wayland-only winit loop and softbuffer surface, drawing a centered integer-cell
frame into the physical inner buffer. The receiver keeps the ScreenCast session and authorized
PipeWire remote alive through capture, negotiates CPU-mapped packed RGB and hands the latest
validated luma frame to the existing locator/receiver. FEC/hash/output run after dispatch returns
from C callbacks, whose errors/panics are contained. Stream size changes invalidate the automatic
location; reacquisition preserves the manifest and generations. Terminal commands are bounded,
source reads stay random-access, and output/preview/stats use create-new semantics. See
[Linux Wayland](linux-wayland.md) for coordinates, system dependencies and pending desktop checks.

API references: [StretchDIBits and bitmap functions](https://learn.microsoft.com/en-us/windows/win32/gdi/bitmap-functions),
[DPI awareness](https://learn.microsoft.com/en-us/windows/win32/hidpi/setting-the-default-dpi-awareness-for-a-process),
[ShowWindow startup behavior](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-showwindow).

## Verification layers

1. Protocol golden vectors and malformed-packet checks.
2. FEC deterministic golden symbols, loss, duplicate, reorder and repair-only recovery.
3. Visual codec round trips at capacity, calibration and malformed dimensions; desktop location,
   movement, fractional scaling, noise, occlusion and session selection.
4. Seeded transport: frame drop, duplicate, cell corruption, bilinear resize, box blur,
   brightness, contrast, noise and batch reorder.
5. End-to-end byte equality and SHA-256 through both profiles and late joining; 1 MiB visual test.
6. Real local screen transfer in separate Windows sender/receiver processes.
7. Real remote desktop tests, kept separate from simulator/local results.

Run `python scripts/check.py` before each logical commit. It runs script regressions followed by
fmt/check/clippy/test on Windows or Linux and fails at the first failing stage.
Local E2E needs an unlocked interactive Windows desktop and permission to
capture the sender region. Its generated fixtures, logs and previews live under ignored `output/`.
