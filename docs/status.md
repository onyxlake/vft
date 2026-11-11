# Support and limitations

The workspace package version is **0.1.0**. Packet/manifest and luminance formats are **v0**;
optional manual repair text uses **VFR1**. The Rust baseline is **1.99.1**, edition **2021**.
See [dependencies](dependencies.md) for the pinned versions and dated validation record.

## Implemented features

| Component | Capabilities |
| --- | --- |
| Shared cores | Explicit wire encoding, CRC32C, SHA-256, deterministic 64-symbol FEC generations, independent binary/four-level visual frames and simplex sessions |
| Windows 10/11 x64 | Win32/GDI sender and physical desktop receiver; manual crop, local sender-window capture and automatic discovery/reacquisition |
| Linux Wayland | winit/softbuffer sender and ScreenCast portal/PipeWire receiver; selected-stream physical coordinates |
| Browser sender | Offline single-file HTML, bounded Worker file reads, WASM core and physical-pixel Canvas presentation |
| Native terminal controls | Default interactive TUI, ordinary CLI mode, optional human-entered repair requests and verified output paths |

Transfers tolerate loss, duplication, reordering and late joining without required feedback.
The sender reads files on demand with one cached generation. Receivers are memory-backed and
save output only after SHA-256 verification, using create-new semantics. Their default 256 MiB
file limit is separate from the 1,000,000-symbol limit; see [resource limits](cli.md#resource-limits).

## Validation evidence

Measurements describe the revisions and environments recorded in each report. Earlier Rust
versions and historical commands do not define the current build requirements. A compilation
or headless recovery pass does not establish physical screen or remote transport acceptance.

| Layer | Recorded evidence | Remaining boundary |
| --- | --- | --- |
| Current toolchain baseline | [2026-11-11 checks](dependencies.md): Linux workspace checks, core native/WASM checks, Windows GNU cross-checks, release build, WASM/native recovery and headless browser checks | Native Windows execution and physical capture were not rerun for that dependency upgrade |
| Local Windows screen | [1 KiB/1 MiB/100 MiB](benchmarks.md), repair-only joining, robust capture, movement/reacquisition; [output destination modes](benchmarks/receiver-output-20261109.md) | These are local measurements at earlier revisions |
| Real Windows RDP | [Controlled gateway lab](benchmarks/remote-rdp-lab-20261002.md): independent SHA matches for 1 MiB and 100 MiB, plus repair-only joining | The full profile/FPS/size matrix and repetitions remain incomplete |
| Local Linux screen | [Hyprland 1 KiB/1 MiB](benchmarks/local-hyprland-20261016.md) and [Kitty TUI 1 MiB](benchmarks/linux-tui-20261016.md), with independent SHA comparison | Broader portal backends, robust/large-file cases, fractional scaling, cross-platform capture and remote acceptance remain |
| Browser sender | [Compiled WASM recovery](benchmarks/wasm-sender-20261002.md), [offline Chromium/Firefox UI checks](benchmarks/web-demo-20261111.md) and subsequent dependency-baseline checks | Browser-to-native physical capture, real background-tab scheduling, Safari and remote acceptance remain |
| Manual repair | [Windows ConPTY/local screen](benchmarks/manual-repair-tui-20261002.md) and Linux Kitty controls | Real remote comparisons, transcription time, broader terminals and exhaustive resize/error handling remain |

The RDP lab's baseline balanced 1 MiB runs reached 52.0 KiB/s. Its sustained 100 MiB transfer
took 3499.663 seconds at 29.3 KiB/s (30.0 decimal KB/s), below the **40–100 decimal KB/s** target.
Standard 8/16-pixel presets, remaining large-file configurations, repeated trials, deliberate
remote movement/occlusion and measured codec/video bitrate still need evidence. The full remote
acceptance criteria are in [remote validation](remote-validation.md).

## Unsupported features and constraints

- Capture requires a complete axis-aligned frame with at least two physical pixels per cell.
  Rotation, camera perspective and partial-frame recovery are unsupported.
- Capture images are limited to 8192 pixels per dimension. Linux needs a CPU-mapped packed RGB
  stream; X11, DMA-BUF-only capture and HDR are unsupported.
- Receiver memory grows with the file. Disk-backed recovery and a live preview window are not
  implemented; `--preview` writes a single diagnostic BMP.
- The FEC is a dense binary fountain code, not RaptorQ, and has no fixed packet-count recovery
  guarantee. Requested FPS is not measured screen or delivered unique-frame FPS.
- CRC/SHA detect corruption but provide no sender authentication or encryption. Archives,
  automatic feedback/profile adaptation, network transport, Android and browser receivers are
  not implemented. Browser manual-repair controls are also unavailable.
- Keep source files unchanged, visual frames unobscured and output/diagnostic paths new.
  Native TUI results remain open until Quit; automation should use `--no-tui`.
