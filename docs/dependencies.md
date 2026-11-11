# Toolchain and dependency baseline

Baseline: source revision `8560c7e`, validated on 2026-11-11. Rust 1.99.1 is the workspace
minimum and the toolchain pinned in `rust-toolchain.toml`, including rustfmt, Clippy and
`wasm32-unknown-unknown`.
The workspace remains on edition 2021. Earlier Rust 1.85 validation records describe
historical revisions; they do not apply to this dependency baseline.

| Direct dependency | Version | Use |
| --- | --- | --- |
| sha2 | 0.11.1 | File integrity and manifest tags |
| Ratatui | 0.30.2 | Native terminal UI |
| Crossterm | 0.29.0 | Native terminal input and state |
| getrandom | 0.4.3 | Linux session identifiers |
| ASHPD | 0.14.0 | Linux ScreenCast portal |
| Tokio | 1.54.1 | Linux portal asynchronous runtime |
| pipewire-rs | 0.10.1 | Linux captured video buffers |
| windows | 0.62.2 | Win32/GDI adapters |
| winit | 0.30.13 | Linux Wayland window and input |
| softbuffer | 0.4.8 | Linux software presentation |

[`Cargo.lock`](../Cargo.lock) pins the complete dependency graph. Desktop dependencies stay
target-specific and outside the five `no_std + alloc` cores.
ASHPD enables only Tokio and ScreenCast. Ratatui explicitly selects the Crossterm 0.29 backend
and its optional layout cache. The browser adapter uses the shared Rust libraries and has no
external JavaScript runtime dependencies. Its generated HTML embeds dependency license notices;
keep those notices when redistributing it.

The bundle build's Cargo metadata query can fetch uncached dependency sources. The resulting
HTML makes no external runtime requests. See [browser build instructions](wasm-sender.md).

## Dated advisory audit

An OSV batch query for every `(name, version)` registry entry in the complete lockfile,
including other-target dependencies, returned **zero advisory matches across 304 packages**
on 2026-11-11. No advisory was suppressed or ignored. The query covers reported advisories
known to OSV on that date; it is not proof that every dependency is free of defects.
The audited lockfile SHA-256 is
`736e3896fd523ee9920d9203f63216485df0a75054c43dd658f7a8339e5dad53`.
Local audit records are under ignored `output/dependency-upgrade-20261111/` in
`osv-after.json` and `audit-summary.json`; generated evidence is not shipped in the repository.

The audit can be repeated by submitting each registry package in `Cargo.lock` to
`https://api.osv.dev/v1/querybatch` with ecosystem `crates.io`, package name and exact version.
Review all returned IDs, including maintenance and soundness notices.

## Validation at this revision

The Rust 1.99.1 baseline passed these checks on Linux:

- `python3 scripts/check.py`: 19 Python regressions, formatting, workspace check,
  strict Clippy and 65 Rust tests, including existing wire/FEC golden vectors.
- The five core libraries with `--no-default-features`, both natively and on
  `wasm32-unknown-unknown`.
- Locked Windows GNU cross-target workspace/all-target checks and strict Clippy
  for both Windows applications.
- `cargo build --release --workspace --locked` for the Linux host.
- `python3 scripts/check-web.py`: packaged WASM/native recovery for both profiles,
  including loss, duplicates, reordering and repair-only joining, with SHA-256 verification.
- Headless Chromium and Firefox: offline `file://` startup, sending, pause/restart,
  narrow layout and embedded license notices.

Physical desktop capture, native Windows execution and real remote acceptance were not rerun.
The [recorded acceptance limits](status.md#validation-evidence) remain. Validation logs are
under the same ignored audit directory.
