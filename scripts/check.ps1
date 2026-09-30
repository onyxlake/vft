$ErrorActionPreference = 'Stop'
cargo fmt --all -- --check
if ($LASTEXITCODE) { exit $LASTEXITCODE }
cargo check --workspace
if ($LASTEXITCODE) { exit $LASTEXITCODE }
cargo clippy --workspace --all-targets -- -D warnings
if ($LASTEXITCODE) { exit $LASTEXITCODE }
cargo test --workspace
exit $LASTEXITCODE
