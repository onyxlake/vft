# VFT agent guidance

These instructions apply to the entire repository. Follow the user's current request and preserve existing user changes.

## Start here

- Inspect `git status --short` and the relevant diff before editing.
- Read [README.md](README.md) for usage and [docs/status.md](docs/status.md) for current progress and limitations.
- The accepted scope and checkpoint workflow are in [docs/plan.md](docs/plan.md). Consult [docs/architecture.md](docs/architecture.md), [docs/protocol.md](docs/protocol.md), and [docs/visual-format.md](docs/visual-format.md) for changes to their respective components.
- Work in small, testable logical units. Keep changes focused; do not overwrite, stage, or revert unrelated work.
- Update documentation when behavior, wire format, commands, milestone evidence, or limitations change.

## Architecture and scope

VFT transfers files through independent visual frames over a simplex channel. Successful recovery must not require ACKs, a shared filesystem, a network connection between endpoints, or capture-order continuity.

| Component | Responsibility |
| --- | --- |
| `crates/vft-protocol` | Explicit packet/manifest encoding, CRC32C, SHA-256, validation |
| `crates/vft-fec` | Deterministic source/repair symbols and generation decoding |
| `crates/vft-visual` | Binary/four-level frame layout, rendering, calibration and sampling |
| `crates/vft-sender` | Random-access object source and manifest/source/repair scheduling |
| `crates/vft-receiver` | Session discovery, symbol collection, recovery and verified completion |
| `crates/vft-testkit` | Seeded simulation and headless integration tooling |
| `apps/vft-send-win` | File adapter, CLI and native Win32/GDI presentation |
| `apps/vft-recv-win` | Physical screen capture, CLI, diagnostics and verified file output |
| `apps/vft-send-web` | WASM adapter, bounded Worker File API source and physical-pixel Canvas presentation |

The five protocol/FEC/visual/sender/receiver libraries are `no_std + alloc`. Keep filesystem, OS, capture and UI APIs in adapters. `vft-testkit` may use `std`. Keep Windows dependencies target-specific.

Respect the workspace's declared Rust version and edition in `Cargo.toml`. A successful build on a newer compiler does not establish minimum-version compatibility. Avoid adding dependencies or platform requirements without a concrete need.

Phase 1 targets Windows 10/11 x64. Additional platforms, camera localization, feedback adaptation, disk-backed recovery, encryption, archives and other extensions are outside the current implementation scope unless requested. See [docs/future-platforms.md](docs/future-platforms.md) before changing portability boundaries.

The user-requested WASM sender is now included. Keep file reads and hashing bounded in its dedicated Worker, reuse the existing sender/visual cores, and preserve pause/stop/hidden-page handling and exact Canvas sizing. See [docs/wasm-sender.md](docs/wasm-sender.md) for its ABI and browser requirements.

## Correctness requirements

- Treat packet and manifest bytes as an explicit wire format. Preserve little-endian encoding, strict version/type/profile/reserved-field validation, checked lengths and allocation limits. Never serialize Rust memory layouts directly.
- Validate packet CRC before accepting symbols. Enter completed state and publish recovered output only after full-file SHA-256 verification. Hash the original file length, excluding symbol padding.
- Preserve independent frame decoding and recovery under loss, duplicates, reordering, corruption and late joining. Repeated or dependent FEC equations must be harmless.
- Keep FEC behavior deterministic across platforms. Seeds, symbol indices, wrapping arithmetic and bit selection must match the documented algorithm; do not use platform RNGs or unspecified iteration order. The current FEC uses 64-symbol generations and is not RaptorQ.
- Change wire/FEC/profile semantics deliberately. Update compatibility identifiers where required, specifications and independently derived golden vectors together. Do not regenerate expected bytes merely to make a failing test pass.
- Keep sender reads behind `ObjectSource::read_exact_at`, with bounded generation caching. Do not load the whole source into RAM. The input must remain unchanged throughout transmission.
- The receiver is currently memory-backed with a default 256 MiB file limit. Preserve input/resource limits and avoid unnecessary full-file copies; do not describe it as bounded-memory streaming recovery.
- Preserve create-new output semantics for recovered files, statistics and previews. Never silently overwrite existing files.
- Keep unsafe operations explicit and justified. In Windows adapters, preserve RAII ownership, Win32 callback reentry guards, panic containment at FFI boundaries, and window/state teardown order.
- Capture coordinates are physical pixels. Preserve DPI awareness, exclude window borders/title bars, and flush GDI before reading captured pixels. The sampler requires an exact rectangle; `--auto` supplies it through sync/CRC-validated discovery and reacquires the selected session after movement. `--preview` saves a diagnostic crop BMP.

## Verification

Run from the repository root on Windows or Linux with Python 3.9+ (`python3` if needed):

```powershell
cargo fmt --all
python scripts/check.py
git diff --check
```

The check script first runs Python script regressions, then stops at the first failure in:

```powershell
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Do not hide failures, disable lints, weaken assertions, or claim checks passed when they were not run. Add regression coverage for behavior changes: malformed/golden bytes for protocol changes, deterministic and repair-only recovery for FEC changes, and frame round trips/distortions for visual changes. Documentation-only changes need no new tests.

For shared-library dependency or feature changes, also check their portability:

```powershell
cargo check -p vft-protocol -p vft-fec -p vft-visual -p vft-sender -p vft-receiver --no-default-features
```

For application or native integration changes, build release binaries and run the relevant desktop checks:

```powershell
cargo build --release --workspace
python scripts/local-e2e.py --sizes 1024 1048576 --fps 60 --cell-size 4
```

For browser sender changes, additionally run `python scripts/check-web.py` (Rust WASM target and Node.js 22+) and check the relevant browser UI behavior. Its WASM/native headless recovery does not establish browser-to-Windows physical capture or real remote acceptance. Generated static files stay under ignored `output/web-sender/`.

Choose additional cases according to the change, including robust mode, repair-only joining or the 100 MiB case when needed. Large screen transfers take many minutes and are not routine documentation checks. Native tests need an unlocked interactive Windows desktop with capture access and a visible sender. Use `--topmost` when occlusion would invalidate the run.

If the sandbox cannot use the normal Cargo cache, the ignored project-local cache can be selected for that shell:

```powershell
$env:CARGO_HOME = Join-Path (Get-Location) '.build\cargo'
```

Keep generated fixtures, capture images and logs under ignored `output/`; keep build/cache artifacts under `target/` and `.build/`. Commit only intentional small golden vectors and documented measurement records.

## Benchmarks and milestone evidence

- Consult [docs/benchmarks.md](docs/benchmarks.md) for recorded measurements and [docs/remote-validation.md](docs/remote-validation.md) for M14 procedures.
- Simulation, local desktop and real remote desktop results are separate evidence. Local success cannot complete M14 or establish the remote 40–100 KB/s target. Check the current status document before reporting completion.
- Verify recovered bytes with an independent SHA-256 comparison. Report payload goodput only for verified successful runs; failed runs have zero verified goodput.
- Record configuration, elapsed time, units, actual observed frame rate, CPU and memory when available. Requested FPS is not observed unique-frame rate. Leave unmeasured values unknown.
- Interpret counters according to the documented definitions. Source/repair capture counts include duplicates; capture rejection is not video frame loss. Do not invent bitrate, symbol error rate or remote-session settings.
- Preserve failed-run diagnostics and describe relevant limitations honestly. Update status only when the required evidence exists.

## Checkpoints and tool hygiene

The accepted plan authorizes automatic local checkpoint commits after validation, one logical unit per commit. Review the final diff, run the check script, stage only the intended paths, and use a descriptive Conventional Commit such as `feat(fec): ...`, `fix(win-receiver): ...`, `test(e2e): ...` or `docs: ...`.

Do not automatically push, force-push, rebase remote branches, reset unrelated user work, or run `git clean -fdx`. Do not change global Git configuration to work around an execution environment issue.

Development scripts use Python's standard library and fail on subprocess errors. Pass command arguments as lists without a shell. Hide Windows background-helper startup windows; application windows needed for visual transfer must still be visible. Stop only processes started by the current run. Preserve create-new evidence files, and verify recursive removal/move targets stay inside the intended directory. See [docs/development.md](docs/development.md) for platform boundaries and command options.

When finishing, state what changed, which checks ran, and any remaining limitations. Include the local checkpoint commit when created. Never equate implemented code, compiled code, local validation and remote acceptance.
