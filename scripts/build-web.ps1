param([switch]$Debug)
$ErrorActionPreference = 'Stop'
$project = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Push-Location $project
try {
    $buildArgs = @('build', '--locked', '-p', 'vft-send-web', '--target', 'wasm32-unknown-unknown')
    $configuration = 'debug'
    if (-not $Debug) { $buildArgs += '--release'; $configuration = 'release' }
    cargo @buildArgs
    if ($LASTEXITCODE) { throw 'WASM build failed. Install the target with: rustup target add wasm32-unknown-unknown' }
    $destination = Join-Path $project 'output/web-sender'
    New-Item -ItemType Directory -Force -Path $destination | Out-Null
    # Copy only the known distribution files; preserve any unrelated output files.
    foreach ($name in @('index.html', 'style.css', 'app.js', 'worker.js', 'bridge.js')) {
        Copy-Item -LiteralPath (Join-Path $project "apps/vft-send-web/web/$name") -Destination (Join-Path $destination $name) -Force
    }
    Copy-Item -LiteralPath (Join-Path $project "target/wasm32-unknown-unknown/$configuration/vft_send_web.wasm") -Destination (Join-Path $destination 'vft_send_web.wasm') -Force
    Write-Host "Browser sender built: $destination"
    Write-Host 'Serve it with: python -m http.server 18780 --bind 127.0.0.1 --directory output/web-sender'
} finally { Pop-Location }
