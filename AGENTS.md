# VFT agent guidance

These instructions apply to the entire repository. Follow the user's current request and preserve existing user changes.

## Start here

- Inspect `git status --short` and the relevant diff before editing.
- Read [README.md](README.md) for usage and limitations. Consult [docs/architecture.md](docs/architecture.md), [docs/protocol.md](docs/protocol.md), and [docs/visual-format.md](docs/visual-format.md) for changes to their respective components.
- Work in small, testable logical units. Keep changes focused; do not overwrite, stage, or revert unrelated work.
- Keep repository documentation focused on technical references, usage and architecture. Update it when behavior, wire format, commands or limitations change. Keep measurement records, audit logs and run histories under ignored `output/`.

## Architecture and scope

VFT transfers files through independent visual frames over a simplex channel. Successful recovery must not require ACKs, a shared filesystem, a network connection between endpoints, or capture-order continuity.

The five protocol/FEC/visual/sender/receiver libraries are `no_std + alloc`. Keep filesystem, OS, capture and UI APIs in adapters. `vft-testkit` may use `std`. Keep Windows dependencies target-specific.

Respect the workspace's declared Rust version and edition in `Cargo.toml`. A successful build on a newer compiler does not establish minimum-version compatibility. Avoid adding dependencies or platform requirements without a concrete need.

Supported adapters target Windows 10/11 and Linux Wayland with 386/amd64 binaries, plus browser sending. The 386 download label uses Rust's i686 targets. Additional platforms, camera localization, feedback adaptation, disk-backed recovery, encryption, archives and other extensions are outside the current implementation scope unless requested. See [README limitations](README.md#limitations) before changing portability boundaries.

Keep browser sender file reads and hashing bounded in its dedicated Worker, reuse the existing sender/visual cores, and preserve pause/stop/hidden-page handling and exact Canvas sizing. See [docs/wasm-sender.md](docs/wasm-sender.md) for its ABI and browser requirements.

For Linux Wayland adapters, see [docs/linux-wayland.md](docs/linux-wayland.md).
Linux desktop dependencies stay target-specific. Capture coordinates refer to selected-stream
physical pixels. Real desktop testing requires stopping and asking the user first.

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

Keep generated fixtures, capture images, measurement records and logs under ignored `output/`; keep build/cache artifacts under `target/` and `.build/`. Commit only intentional small golden vectors needed by tests.

## Test reporting

- Keep simulation, local desktop and real remote desktop results separate. Local success does not establish remote compatibility or throughput.
- Verify recovered bytes with an independent SHA-256 comparison. Report payload goodput only for verified successful runs; failed runs have zero verified goodput.
- Record configuration, elapsed time, units, actual observed frame rate, CPU and memory when available. Requested FPS is not observed unique-frame rate. Leave unmeasured values unknown.
- Interpret counters according to the documented definitions. Source/repair capture counts include duplicates; capture rejection is not video frame loss. Do not invent bitrate, symbol error rate or remote-session settings.
- Preserve failed-run diagnostics under ignored `output/` and describe relevant limitations honestly.

## Checkpoints and tool hygiene

Automatic local checkpoint commits are authorized after validation, one logical unit per commit. Review the final diff, run the check script, stage only the intended paths, and use a descriptive Conventional Commit such as `feat(fec): ...`, `fix(win-receiver): ...`, `test(e2e): ...` or `docs: ...`.

Do not automatically push, force-push, rebase remote branches, reset unrelated user work, or run `git clean -fdx`. Do not change global Git configuration to work around an execution environment issue.

Development scripts use Python's standard library and fail on subprocess errors. Pass command arguments as lists without a shell. Hide Windows background-helper startup windows; application windows needed for visual transfer must still be visible. Stop only processes started by the current run. Preserve create-new evidence files, and verify recursive removal/move targets stay inside the intended directory. See [docs/development.md](docs/development.md) for platform boundaries and command options.

When finishing, state what changed, which checks ran, and any remaining limitations. Include the local checkpoint commit when created. Never equate implemented code, compiled code, local validation and remote acceptance.
