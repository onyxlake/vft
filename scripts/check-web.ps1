$ErrorActionPreference = 'Stop'
$project = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Push-Location $project
try {
    & (Join-Path $PSScriptRoot 'build-web.ps1')
    cargo clippy -p vft-send-web --target wasm32-unknown-unknown -- -D warnings
    if ($LASTEXITCODE) { throw 'WASM clippy failed' }
    foreach ($name in @('app.js', 'bridge.js', 'worker.js')) {
        node --check (Join-Path $project "apps/vft-send-web/web/$name")
        if ($LASTEXITCODE) { throw "JavaScript syntax check failed: $name" }
    }
    cargo build -p vft-send-web --example verify-frames
    if ($LASTEXITCODE) { throw 'Native frame verifier build failed' }
    node (Join-Path $PSScriptRoot 'web-e2e.mjs')
    if ($LASTEXITCODE) { throw 'WASM/native frame recovery failed' }
} finally { Pop-Location }
