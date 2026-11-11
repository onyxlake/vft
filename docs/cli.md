# Native CLI reference

Run each binary with `--help` for its usage text. Windows executables are under
`target\release\` after a release build; Linux executables are under `target/release/`,
unless Cargo uses a custom target directory. In PowerShell, include a path such as
`.\target\release\vft-send-win.exe`.

## Sender

Supply a source file and keep it unchanged until transmission ends. The sender hashes the file
in bounded chunks and reads source symbols on demand. It sends sources once, then continues
repair frames until stopped; it cannot infer receiver completion.

| Argument | Default | Meaning |
| --- | --- | --- |
| `FILE` | Required | Source file; Linux requires a regular file |
| `--profile robust\|balanced` | `balanced` | Standard visual lattice and luminance levels |
| `--fps N` | `15` | Requested presentation rate, integer 1–60 |
| `--cell-size N` | Balanced: `8`; robust: `16` | Initial cell size, 2–32 physical pixels |
| `--fullscreen` | Off | Start fullscreen |
| `--frames N` | Unlimited | Positive frame-count limit: generated frames on Windows, submitted advancing frames on Linux; not a delivery guarantee |
| `--tui` / `--no-tui` | Automatic | Require the TUI / force ordinary CLI; mutually exclusive |

Windows also supports:

| Argument | Default | Meaning |
| --- | --- | --- |
| `--topmost` | Off | Keep the visual window above ordinary windows |
| `--position X Y` | `40 40` | Initial window position in physical desktop coordinates |
| `--rect-file PATH` | None | Reserve a new rectangle telemetry file before opening the window |
| `--test-pattern` | Off | Show a test pattern instead of a file; mutually exclusive with `FILE` |

The rectangle file starts empty and is updated through its retained handle when the painted
client rectangle changes. Existing files, including source-file aliases, are rejected. Readers
must wait for a complete newline-terminated `X Y WIDTH HEIGHT` record. This local diagnostic
aid is not required by automatic visual discovery.

Windows scales its framebuffer to the client area on resize/fullscreen; record the displayed
dimensions when measuring physical cell sizes. Linux centers an integer-cell grid, reducing cell
size to fit and pausing when fewer than two physical pixels per cell fit. Wayland provides no
equivalent of the Windows positioning/topmost options.

The visual window accepts Esc to exit, Space to pause/resume and F11 to toggle fullscreen.
See [terminal controls](terminal-controls.md) for the separate TUI.

## Receiver

Select exactly one capture mode:

| Argument | Windows | Linux Wayland |
| --- | --- | --- |
| `--auto` | Locate a visible VFT frame on the virtual desktop | Locate a VFT frame in the selected portal stream |
| `--rect X Y WIDTH HEIGHT` | Exact frame crop in physical desktop pixels | Exact frame crop in selected-stream physical pixels |
| `--sender-window` | Follow a local native VFT sender's client area | Unavailable |

Other options:

| Argument | Default | Meaning |
| --- | --- | --- |
| `--output FILE` | Manifest filename | Explicit destination; overrides `--output-dir` |
| `--output-dir DIR` | Current directory | Existing directory for the manifest filename |
| `--search-rect X Y WIDTH HEIGHT` | Entire desktop/stream | Restrict discovery; requires `--auto` |
| `--profile auto\|robust\|balanced` | `auto` | Restrict decoding to a profile, or try both |
| `--fps N` | `30` | Requested capture/processing ceiling, integer 1–120 |
| `--timeout SECONDS` | `3600` | Positive integer wall-time limit; Linux includes portal selection |
| `--max-file-size BYTES` | `268435456` | Maximum manifest file length; `0` permits only empty files |
| `--preview FILE.bmp` | None | One diagnostic crop; Windows saves the first captured/located crop, Linux the first successfully decoded crop |
| `--stats FILE.csv` | None | Save transfer counters and verified goodput |
| `--tui` / `--no-tui` | Automatic | Require the TUI / force ordinary CLI; mutually exclusive |

The complete frame must be visible, axis-aligned and at least two physical pixels per cell.
Exclude title bars and borders from manual crops. Automatic discovery searches at most about
twice per second; after 750 ms of continuous decode failure it reacquires while preserving the
selected session and recovery progress. It supports only the two standard lattices.

Windows supports negative monitor origins. Capture/search dimensions must be 192×108 through
8192×8192 pixels; balanced needs at least 384×216 pixels to sample its grid. Use `--search-rect`
if the combined desktop exceeds the maximum dimensions. Without a selected manifest, the first
CRC-valid discovered frame determines the initial location.

Linux coordinates are relative to `(0, 0)` in the selected PipeWire stream, not global/logical
desktop coordinates. Manual/search rectangles must be at least 192×108 pixels, with nonnegative
origins and right/bottom edges within 8192. Balanced still needs at least 384×216 pixels.
The complete stream must fit within 8192×8192 even when restricting search.
See [Linux capture requirements](linux-wayland.md#capture-and-recovery).

## Output rules

For a manifest filename of `photo.png`:

| Options in addition to the capture mode | Destination |
| --- | --- |
| None | `./photo.png` |
| `--output-dir ./received` | `./received/photo.png` |
| `--output renamed.png` | `./renamed.png` |
| `--output-dir ./received --output renamed.png` | `./renamed.png` |

Explicit output paths retain their supplied meaning; relative paths use the current working
directory. With `--output`, the unused `--output-dir` is ignored even if it does not exist.
All destination parent directories must already exist. The receiver reports the resolved output
after manifest acceptance and on completion.

Metadata filenames must meet the [protocol rules](protocol.md#manifest-payload). When using that
name for output, Windows additionally rejects device names, invalid characters, control characters
and trailing dots/spaces. An explicit `--output` can supply a local name for metadata that is
protocol-valid but unsuitable for the receiving filesystem. CLI/TUI metadata is escaped for
terminal control characters; the transmitted bytes are unchanged.

Recovered output, preview and stats paths must be distinct and must not already exist. Recovered
bytes are written only after full-file SHA-256 verification. Final file creation uses create-new
semantics, including protection against a destination created after manifest acceptance.
CRC and SHA-256 detect corruption; neither authenticates the sender or encrypts the contents.

## Resource limits

Both receivers retain recovery data in memory. The default file limit is 256 MiB; this is a
file-length limit, not a process-memory cap. Frame buffers, FEC state and metadata add overhead.

`--max-file-size` adjusts only the file-length limit. Native receivers keep the core's separate
1,000,000-source-symbol limit, which has no CLI override. With the standard sender profiles:

| Profile | Symbol payload | File-length ceiling from the symbol limit |
| --- | --- | --- |
| Robust | 531 bytes | 531,000,000 bytes (about 506.4 MiB) |
| Balanced | 4,884 bytes | 4,884,000,000 bytes (about 4.55 GiB) |

The effective limit is the smaller of `--max-file-size` and the symbol ceiling. For example,
a 512 MiB robust transfer still exceeds the symbol ceiling after raising the byte limit.
These are acceptance bounds, not tested maximum file sizes or promises of sufficient memory.
Core callers can configure `vft_receiver::Limits`; applications must budget memory before
raising either bound. Disk-backed recovery is not implemented.
