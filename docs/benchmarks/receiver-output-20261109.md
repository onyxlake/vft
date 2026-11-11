# Native receiver output destinations (2026-11-09)

Source revision: `5b74ee5`. Both native receivers support `--output-dir DIR`, defaulting to
the current working directory, and use the manifest filename when `--output` is omitted. Explicit output paths
take precedence. The directory must exist; unsafe metadata names, existing destinations
and preview/statistics aliases are rejected. SHA verification and create-new writes remain
required. No wire/FEC/visual changes or external dependencies were added.

## Checks

Windows x64: Windows `10.0.26300.0`, Rust 1.98.1, Python 3.14.7.
Linux headless: Arch Linux under WSL, Rust 1.95.0, PipeWire headers 1.6.9, Clang 22.1.8.
`python scripts/check.py` passes on both platforms: 15 Python regressions followed by
fmt, workspace check, strict all-target Clippy and workspace tests. Release workspace
builds pass on both platforms. Windows additionally passes
`cargo +1.85.0 check --workspace --all-targets --locked`.
The six redirected CLI/TUI probes pass.

New regression coverage checks default/current-directory naming, selected directories,
explicit overrides (including an unused nonexistent directory), unsafe filenames,
existing destinations, diagnostic aliases, Windows device/case rules and Unix symlink
aliases/dangling destinations. Native adapter tests verify no recovered output before
SHA completion and preserve files created after manifest acceptance; Linux also checks
that progress/completion retain the resolved path and correct lifecycle state.

## Windows physical capture

Release binaries, balanced profile, requested sender FPS 60, receiver FPS 120, four-pixel
cells, `--sender-window`, `--topmost`, no join delay, capture rectangle `48 71 768 432`.
Each case independently compares source and recovered SHA-256. This is local screen
capture evidence, separate from Linux desktop and real remote acceptance.

```text
python scripts/local-e2e.py --sizes 1024 1048576 --fps 60 --cell-size 4 --timeout-seconds 120 --skip-build
python scripts/local-e2e.py --sizes 1024 1048576 --fps 60 --cell-size 4 --timeout-seconds 120 --output-mode metadata --skip-build
python scripts/local-e2e.py --sizes 1024 1048576 --fps 60 --cell-size 4 --timeout-seconds 120 --output-mode current-dir --skip-build
python scripts/check-tui-cli.py --skip-build
```

| Destination mode | Bytes | Receiver elapsed (s) | Verified B/s | Observed adjacent unique frames/s |
| --- | ---: | ---: | ---: | ---: |
| Explicit path | 1,024 | 0.649711 | 1,576.086 | 32.322 |
| Explicit path | 1,048,576 | 10.771834 | 97,344.241 | 31.935 |
| Metadata + selected directory | 1,024 | 0.746152 | 1,372.375 | 32.165 |
| Metadata + selected directory | 1,048,576 | 10.627251 | 98,668.600 | 31.899 |
| Metadata + working directory | 1,024 | 0.651946 | 1,570.681 | 32.211 |
| Metadata + working directory | 1,048,576 | 10.687192 | 98,115.205 | 32.001 |

The [recorded CSV](receiver-output-20261109.csv) includes measured process CPU seconds,
peak working-set bytes and capture counters. Adjacent unique-frame rate counts valid
packets differing from the previous valid packet; it is not observed display FPS or
video frame loss. Remote transport settings and codec/video bitrate are unmeasured.

Independent source/output SHA-256 values for all three modes:

```text
1024:    96BC8927FC9F5B5D566937A6EBFAFDFD0B9D0394F08D1FB6EF10FB396D08E8AE
1048576: D8A01F8A1538C5AF49888285F88824683809D3D1CCBA7EAF343BCD82E565604B
```

Raw fixtures, logs, previews, stats and summaries are retained under ignored `output/`:

- `local-20261109-070737-0jy02khm` (explicit)
- `local-20261109-070849-xi_f18jn` (metadata + selected directory)
- `local-20261109-071003-qg1phn90` (metadata + working directory)
- `tui-cli-20261109-071003-yhifm8xb` (redirected mode probes)

Linux portal/desktop capture and real remote transfers were not rerun for this change.
These local results do not establish remote goodput or complete remote acceptance.
