# Terminal controls and manual repair

The Windows and Linux native applications share an English terminal UI. Interactive stdin and
stdout select it automatically. `--no-tui` forces ordinary CLI output; `--tui` requires both
streams to be interactive. The flags are mutually exclusive. Help and argument errors remain
ordinary text. The browser sender has separate [page controls](wasm-sender.md).

Use at least an 80×24 terminal; 100×28 gives more space. Keep it outside the visual frame.
The sender's visual window independently accepts Esc, Space and F11. Terminal raw mode,
alternate screen, cursor and bracketed-paste state are restored on normal exit, errors and
unwind; forced process termination cannot guarantee restoration.

## Receiver

| Key | Action |
| --- | --- |
| Up / Down | Move within the incomplete-group table |
| PgUp / PgDn | Change table page |
| Space | Toggle the highlighted group |
| A / C | Select the current page / clear selection |
| R | Freeze a new repair request for selected groups, or all incomplete groups if none are selected |
| Tab | Switch between table and request; return from the copy view |
| Left / Right | Change request-code page |
| Up / Down, PgUp / PgDn in request or finished view | Scroll the frozen text or final result |
| P | Open the copy view, with each full code on one logical terminal-wrapped line |
| Q / Ctrl+C | Exit |

Group numbers begin at 1. A request freezes the current rank deficits; capture and recovery
continue while it is displayed. Each full code contains at most 256 targets. With no explicit
selection, larger sets of incomplete groups are split into multiple codes. R generates a fresh
snapshot and new request IDs. Manually select/copy the text or transcribe group numbers; clipboard
sharing is optional.

Verified completion and the output path remain visible until Quit, including when returning from
the copy view. Ordinary CLI exits automatically. A completed manual batch at the sender is not
proof that the receiver has completed.

## Sender

| Key | Action |
| --- | --- |
| R | Open the repair-request editor |
| Enter in the editor | Validate and preview; press again to apply |
| Esc in the editor | Close the editor |
| Left / Right, Home / End, Backspace / Delete | Edit input |
| Up / Down in the editor | Scroll the preview |
| Space outside the editor | Pause/resume transmission |
| X outside the editor | Cancel the active batch's unsent repair budget |
| Q outside the editor / Ctrl+C | Exit |

Editing does not pause transmission. Inside the editor, ordinary shortcuts become text;
Ctrl+C still exits. Only one manual batch can be active at a time. Cancellation preserves
already sent packet indices, and normal transmission continues at its own position.

| Example | Meaning |
| --- | --- |
| `18,42,90-91` | Sixteen new repairs for each listed group |
| `18:3,42:1` | Rank deficits of 3 and 1; budgets of nine and eight repairs |
| A full `VFR1/...` code | Manifest-bound request with ID and CRC validation |

Simple lists require checking the current file and manifest tag yourself. Full codes check the
current tag, and repeated identical accepted IDs are no-ops within the recent 64-ID window.
Conflicting content for the same retained ID is rejected. Groups must exist, ranges must not
overlap, and deficits must be between 1 and the group's actual source count. Input is limited
to 4096 bytes and 256 expanded targets. See the [VFR1 contract](protocol.md#optional-manual-repair-text-vfr1).

Manual repair is optional: recovery still works without a return channel. Budgets are heuristics
and may require another request.
