param([switch]$SkipBuild)
$ErrorActionPreference = 'Stop'
$project = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
if (-not $SkipBuild) {
    Push-Location $project
    try { cargo build --release --workspace; if ($LASTEXITCODE) { throw 'Build failed' } }
    finally { Pop-Location }
}
$run = Join-Path $project ('output/tui-cli-' + [DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss') + '-' + [guid]::NewGuid().ToString('N').Substring(0,8))
New-Item -ItemType Directory -Path $run | Out-Null
function Invoke-Probe([string]$Name, [string]$App, [string[]]$Arguments, [int]$Expected, [string]$Message) {
    $stdout = Join-Path $run ($Name + '.log')
    $stderr = Join-Path $run ($Name + '.err')
    $exe = Join-Path $project ('target/release/' + $App + '.exe')
    $process = Start-Process -FilePath $exe -ArgumentList $Arguments -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    try {
        if (-not $process.WaitForExit(15000)) { throw "$Name timed out" }
        if ($process.ExitCode -ne $Expected) { throw "$Name exit code $($process.ExitCode), expected $Expected" }
        $text = (Get-Content -LiteralPath $stdout -Raw) + (Get-Content -LiteralPath $stderr -Raw)
        if ($text -and $text.Contains([string][char]27)) { throw "$Name wrote terminal escapes to redirected output" }
        if ($Message -and -not $text.Contains($Message)) { throw "$Name missing expected English context: $Message" }
        Write-Output "PASS $Name"
    } finally {
        if (-not $process.HasExited) { Stop-Process -Id $process.Id -Force }
        $process.Dispose()
    }
}
Invoke-Probe 'sender-default-redirected' 'vft-send-win' @('--test-pattern','--cell-size','2','--fps','60','--frames','2') 0 ''
Invoke-Probe 'sender-no-tui' 'vft-send-win' @('--test-pattern','--cell-size','2','--fps','60','--frames','2','--no-tui') 0 ''
Invoke-Probe 'sender-explicit-tui-redirected' 'vft-send-win' @('--test-pattern','--tui') 1 'TUI requires interactive stdin and stdout'
Invoke-Probe 'sender-conflicting-modes' 'vft-send-win' @('--test-pattern','--tui','--no-tui') 1 'mutually exclusive'
$output = '"' + (Join-Path $run 'unused.bin') + '"'
Invoke-Probe 'receiver-explicit-tui-redirected' 'vft-recv-win' @('--auto','--output',$output,'--tui') 1 'TUI requires interactive stdin and stdout'
Invoke-Probe 'receiver-conflicting-modes' 'vft-recv-win' @('--auto','--output',$output,'--tui','--no-tui') 1 'mutually exclusive'
Write-Output "Evidence: $run"
