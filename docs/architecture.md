# Architecture

## Components

| Component | Responsibility |
| --- | --- |
| `crates/vft-protocol` | Explicit packet/manifest encoding, CRC32C, SHA-256 and validation |
| `crates/vft-fec` | Deterministic source/repair symbols and generation decoding |
| `crates/vft-visual` | Binary/four-level layout, rendering, calibration, sampling and location |
| `crates/vft-sender` | Random-access source and manifest/source/repair scheduling |
| `crates/vft-receiver` | Session discovery, symbol collection, recovery and hash verification |
| `crates/vft-testkit` | Seeded distortion simulation and headless integration tooling |
| `apps/vft-send-win` / `apps/vft-recv-win` | Win32/GDI presentation, physical capture and file adapters |
| `apps/vft-linux` | Shared Linux file, pixel and recovery adapters |
| `apps/vft-send-linux` / `apps/vft-recv-linux` | Wayland presentation and ScreenCast/PipeWire capture |
| `apps/vft-send-web` | Bounded Worker File API source, WASM core and Canvas presentation |
| `apps/vft-tui` | Shared native terminal models, controls and rendering |

```text
File → sender → FEC symbols → protocol packets → visual frames → screen
Screen capture → visual sampling → CRC-validated packets → receiver/FEC → SHA-256 → output
```

`vft-protocol`, `vft-fec`, `vft-visual`, `vft-sender`, and `vft-receiver` are `no_std + alloc`.
They use no filesystem, Windows, UI or capture APIs. `vft-testkit` uses std for test tooling.
The only external shared dependency is SHA-256 (`sha2` with default features disabled).
Windows APIs are target-specific dependencies of the two applications.

## Terminal integration

`apps/vft-tui` supplies common English control models, Unicode input editing, bounded logs and a
Windows/Linux Ratatui/Crossterm backend. Native apps choose TUI once at startup when
stdin and stdout are interactive; `--no-tui` preserves script behavior and explicit `--tui`
requires both terminals. Help and argument errors remain ordinary text.

The sender's Win32 thread owns SenderSession, HWND and GDI. A terminal thread sends small commands
through a 32-slot queue and posts a private wake message; changes occur at guarded message
boundaries. Stop has a separate atomic signal, including while paused. The receiver creates and
destroys Capture and ReceiverSession on its capture worker; the main thread owns its terminal.
Progress replaces a latest snapshot, with 5 Hz rendering, six-row incomplete-group pages and a
200-line log ring. Request text is frozen at a capture boundary and paginated into 256-target
codes with OS-random IDs. No pixels or recovered file copies cross into the terminal.

The Windows receiver notifies the UI immediately before hashing. Native Complete is published only
after SHA verification and create-new output succeed. TUI results remain visible until Quit,
while CLI completion still exits. RAII restores raw mode, alternate screen, cursor and bracketed
paste after normal exits, errors and unwind; workers are stopped and joined before final summaries.
Forced process termination cannot restore a terminal. The independent visual window remains the
transport and must be kept clear of the control terminal. Manual text does not create an automated
feedback channel or imply receiver completion at the sender.

## Source and recovery storage

Sender input implements `ObjectSource::read_exact_at`. The Windows adapter keeps an open file,
hashes it in 64 KiB chunks, and seeks on demand. Encoder caching retains only one 64-symbol
generation (about 305 KiB for balanced), plus frame buffers. The browser adapter supplies the same
random-access trait from a dedicated Worker using FileReaderSync on bounded File/Blob slices.
It hashes in 64 KiB chunks, transfers one logical frame at a time to the main thread, and renders
it through nearest-neighbor Canvas scaling with physical-pixel/DPI sizing. Pause/hidden-page handling
keeps the current session; stop terminates the Worker. Its small scalar WASM ABI requires no new
binding dependencies. See [wasm-sender.md](wasm-sender.md) for build, lifecycle and ABI contracts.
The file must remain unchanged for the transfer. No ACK, completion feedback or shared filesystem
is required by either core.

Receiver FEC uses one lazily allocated decoder per incomplete generation. Completed generation
bytes stay in memory, allowing ordered SHA-256 validation and verified output chunks without a
second full-file copy. The receiver is memory-backed, with a 256 MiB default file limit and a separate
1,000,000-symbol limit; disk-backed recovery is not implemented. See [resource limits](cli.md#resource-limits).
`reconstruct()` explicitly makes a full copy; `verified_chunks()` avoids it. Sender streaming
support does not imply bounded receiver memory. Gaussian elimination and symbol generation
have bounded 64-symbol algebraic complexity.

## Windows adapters

The Windows sender paints a software BGRA framebuffer in one `StretchDIBits` call. Its message
callback holds a stable boxed `RefCell` for state, catches panic at the FFI boundary, and guards
against synchronous Win32 message reentry.
The optional `--topmost` mode keeps the window above ordinary windows; overlays or other topmost
windows can still obscure it.
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

API references: [StretchDIBits and bitmap functions](https://learn.microsoft.com/en-us/windows/win32/gdi/bitmap-functions),
[DPI awareness](https://learn.microsoft.com/en-us/windows/win32/hidpi/setting-the-default-dpi-awareness-for-a-process),
[ShowWindow startup behavior](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-showwindow).

## Output validation

Both native receivers include the std-only `apps/receiver_output.rs` module for destination
validation, outside the portable cores. `--output-dir` defaults to the current working
directory; without an explicit `--output`, the accepted manifest supplies the filename.
The metadata output directory must exist when used. Explicit output paths take precedence and
retain their supplied meaning. Resolution rejects unsafe metadata names, existing destinations and aliases of
preview/statistics paths, and is reported before recovered output is written. Final file
creation still uses create-new after SHA verification to protect against later collisions.

## Linux adapters

Linux adapter helpers live in `apps/vft-linux`, outside the portable cores. The native
sender uses a Wayland-only winit loop and softbuffer surface, drawing a centered integer-cell
frame into the physical inner buffer. The receiver keeps the ScreenCast session and authorized
PipeWire remote alive through capture, negotiates CPU-mapped packed RGB and hands the latest
validated luma frame to the existing locator/receiver. FEC/hash/output run after dispatch returns
from C callbacks, whose errors/panics are contained. Stream size changes invalidate the automatic
location; reacquisition preserves the manifest and generations. Terminal commands are bounded,
source reads stay random-access, and output/preview/stats use create-new semantics. See
[Linux Wayland](linux-wayland.md) for coordinates, system dependencies and capture constraints.

The [build and test guide](development.md) covers workspace checks and platform-specific harnesses.
