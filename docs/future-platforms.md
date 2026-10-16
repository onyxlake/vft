# Future platforms and scope

Current implementations: shared Rust libraries, Windows 10/11 x64 applications, and the
user-requested [browser WASM sender](wasm-sender.md), and
[Linux Wayland sender/receiver](linux-wayland.md) (desktop acceptance pending).
No Android adapter, WASM receiver, encryption, multi-file archive, ARQ, network transport, plugin system,
GPU acceleration, QR/JAB compatibility, or full RaptorQ implementation is included.

Public cores accept bytes, random-access input and normalized luminance grids; platform adapters
can be added without changing the wire format or importing Windows APIs into shared crates.

- Linux Wayland sender: implemented with winit + softbuffer; receiver: portal + PipeWire.
  Physical desktop and remote acceptance remain pending. X11 fallback is deferred.
- WASM sender: implemented with a Worker FileReaderSync adapter and Canvas presentation;
  browser-to-Windows screen capture and real RDP acceptance remain to be recorded.
- WASM receiver: experimental camera/VideoFrame sampling.
- Android receiver: Kotlin/CameraX Y plane through JNI into the existing Rust sampling/receiver.
- Optional remote filesystem feedback: DONE, progress or profile request. Absence of this feedback
  must continue to support complete transfers. No automated side channel is currently implemented.

Optional human feedback is now supported by the Windows terminal controls: users copy group
lists or manifest-bound VFR1 request codes to the sender. The shared parser and targeted scheduler
remain portable; terminal dependencies and OS random IDs are adapter-only. The browser sender
continues to use automatic scheduling; browser manual-request UI is deferred. No clipboard,
shared filesystem or network feedback is required.

Profile adaptation must follow real codec benchmarks. First measure verified payload goodput,
unique-frame rate, CRC rejection, loss, FEC overhead and CPU across FPS/profile combinations.
Then consider temporal payload layouts, alternative FEC and optional profile requests.

Immediate follow-ups before a broad production release: disk-backed receiver generations,
receiver preview window, explicit Windows manifest/monitor selection,
and additional malformed-input/resource-bound testing. These do not replace M14 remote validation.
