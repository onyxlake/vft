# Architecture

```text
protocol ← fec
    ↑       ↑
  visual ← sender
    ↑       ↑
    └── Windows sender (file source, SHA-256 scan, Win32/DIB)

protocol + fec → receiver
visual + receiver → Windows receiver (physical screen rectangle, GDI capture, file output)
sender + visual + testkit + receiver → headless integration tests
```

`vft-protocol`, `vft-fec`, `vft-visual`, `vft-sender`, and `vft-receiver` are `no_std + alloc`.
They use no filesystem, Windows, UI or capture APIs. `vft-testkit` uses std for test tooling.
The only external shared dependency is SHA-256 (`sha2` with default features disabled).
Windows APIs are target-specific dependencies of the two applications.

Sender input implements `ObjectSource::read_exact_at`. The Windows adapter keeps an open file,
hashes it in 64 KiB chunks, and seeks on demand. Encoder caching retains only one 64-symbol
generation (about 305 KiB for balanced), plus frame buffers. The file must remain unchanged for
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

Run `scripts/check.ps1` before each logical commit. It runs fmt/check/clippy/test and fails at the
first failing stage. Local E2E needs an unlocked interactive Windows desktop and permission to
capture the sender region. Its generated fixtures, logs and previews live under ignored `output/`.
