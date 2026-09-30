# VFT — Visual File Transfer

Simplex file transfer over independent luminance frames. Rust shared libraries and native Windows
sender/receiver; no network transport or required acknowledgements. See [the implementation plan](docs/plan.md).

Build and validate:

```powershell
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Milestone status and operating instructions will be recorded as the implementation progresses.

