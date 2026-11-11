# Toolchain and dependency baseline

Updated 2026-11-11. Rust 1.99.1 is the workspace minimum and the toolchain pinned in
`rust-toolchain.toml`, including rustfmt, Clippy and `wasm32-unknown-unknown`.
The workspace remains on edition 2021. Earlier Rust 1.85 validation records describe
historical revisions; they do not apply to this dependency baseline.

| Direct dependency | Previous | Current |
| --- | --- | --- |
| sha2 | 0.10.9 | 0.11.1 |
| Ratatui | 0.29.0 | 0.30.2 |
| Crossterm | 0.28.1 | 0.29.0 |
| getrandom | 0.2.17 | 0.4.3 |
| ASHPD | 0.11.0 | 0.14.0 |
| Tokio | 1.53.3 | 1.54.1 |
| pipewire-rs | 0.9.2 | 0.10.1 |
| windows | 0.62.2 | 0.62.2 |
| winit | 0.30.13 | 0.30.13 |
| softbuffer | 0.4.8 | 0.4.8 |

These are the current non-yanked stable direct-dependency versions at the review date.
`Cargo.lock` also updates compatible transitive dependencies. Target-specific boundaries
are preserved: desktop dependencies stay outside the five `no_std + alloc` cores.
ASHPD enables only Tokio and ScreenCast. Ratatui explicitly selects the Crossterm 0.29 backend
and retains the layout cache, which became an optional feature in 0.30.

## Follow-up to open-source review item 4

The old lockfile's `lru 0.12.5` has been replaced by `lru 0.18.5` through Ratatui.
This exceeds the patched versions for both reported soundness issues:

- [RUSTSEC-2026-0002](https://rustsec.org/advisories/RUSTSEC-2026-0002.html): fixed in 0.16.3.
- [RUSTSEC-2026-0253](https://rustsec.org/advisories/RUSTSEC-2026-0253.html): fixed in 0.18.2.

`paste`, `zbus_names`, `zvariant` and `zvariant_derive` are absent from the new lockfile.
ASHPD now uses `zbus 6.2.1`, whose integrated wire/name support replaces the retired
standalone crates. See the [zvariant maintenance advisory](https://rustsec.org/advisories/RUSTSEC-2026-0370.html).

An OSV batch query for every `(name, version)` registry entry in the complete lockfile,
including other-target dependencies, returned **zero advisory matches across 304 packages**
on 2026-11-11. No advisory was suppressed or ignored. The query covers reported advisories
known to OSV on that date; it is not proof that every dependency is free of defects.
The response and lockfile SHA-256 are retained locally under
`output/dependency-upgrade-20261111/osv-after.json` and `audit-summary.json`.

The audit can be repeated by submitting each registry package in `Cargo.lock` to
`https://api.osv.dev/v1/querybatch` with ecosystem `crates.io`, package name and exact version.
Review all returned IDs, including maintenance and soundness notices.

## Validation

The Rust 1.99.1 baseline passes these checks on Linux:

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

The new SHA-256 dependency replaces `generic-array` with `hybrid-array`; the generated HTML
retains the current dependency notices. The bundle's metadata query can fetch uncached sources
during the build, while the resulting HTML still makes no external runtime requests.

Physical desktop capture, native Windows execution and real remote acceptance were not rerun.
The existing M14 limitations remain. Logs and audit responses are under ignored
`output/dependency-upgrade-20261111/`.
