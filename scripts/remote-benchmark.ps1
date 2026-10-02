param(
    [Parameter(Mandatory=$true)][int]$X,
    [Parameter(Mandatory=$true)][int]$Y,
    [Parameter(Mandatory=$true)][int]$Width,
    [Parameter(Mandatory=$true)][int]$Height,
    [Parameter(Mandatory=$true)][ValidateSet('robust','balanced')][string]$Profile,
    [Parameter(Mandatory=$true)][int]$SenderFps,
    [Parameter(Mandatory=$true)][int]$CellSize,
    [string]$Transport = 'unspecified',
    [string]$Codec = 'unknown',
    [Nullable[double]]$VideoBitrateMbps = $null,
    [string]$ExpectedSha256,
    [int]$TimeoutSeconds = 3600
)
$ErrorActionPreference = 'Stop'
$project = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$receiverExe = Join-Path $project 'target/release/vft-recv-win.exe'
if (-not (Test-Path -LiteralPath $receiverExe)) { throw 'Build release binaries first' }
$run = Join-Path $project ('output/remote-' + [DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss') + '-' + [guid]::NewGuid().ToString('N').Substring(0,8))
New-Item -ItemType Directory -Path $run | Out-Null
$outputPath = Join-Path $run 'recovered.bin'
$statsPath = Join-Path $run 'stats.csv'
$previewPath = Join-Path $run 'preview.bmp'
$configuration = [ordered]@{
    transport = $Transport; codec = $Codec; video_bitrate_mbps = $VideoBitrateMbps
    requested_sender_fps = $SenderFps; cell_size = $CellSize; profile = $Profile
    capture_rect = @($X,$Y,$Width,$Height); expected_sha256 = $ExpectedSha256
    captured_at_utc = [DateTime]::UtcNow.ToString('o')
}
$configuration | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $run 'configuration.json') -Encoding utf8
$argsForReceiver = @('--rect',"$X","$Y","$Width","$Height",'--no-tui','--profile',$Profile,'--output',('"' + $outputPath + '"'),'--fps','120','--timeout',"$TimeoutSeconds",'--stats',('"' + $statsPath + '"'),'--preview',('"' + $previewPath + '"'))
$receiver = Start-Process -FilePath $receiverExe -ArgumentList $argsForReceiver -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $run 'receiver.log') -RedirectStandardError (Join-Path $run 'receiver.err')
$cpu = 0.0
$peak = 0L
try {
    while (-not $receiver.WaitForExit(1000)) {
        $receiver.Refresh()
        $cpu = $receiver.TotalProcessorTime.TotalSeconds
        $peak = [Math]::Max($peak,$receiver.PeakWorkingSet64)
    }
    $receiver.Refresh()
    $cpu = $receiver.TotalProcessorTime.TotalSeconds
    $peak = [Math]::Max($peak,$receiver.PeakWorkingSet64)
    if ($receiver.ExitCode -ne 0) { throw "Receiver failed; diagnostics: $run" }
    $actual = (Get-FileHash -LiteralPath $outputPath -Algorithm SHA256).Hash
    if ($ExpectedSha256 -and $ExpectedSha256 -ne $actual) { throw 'Input/output SHA-256 mismatch' }
    $stats = Import-Csv -LiteralPath $statsPath
    $stats | Add-Member NoteProperty receiver_cpu_seconds $cpu
    $stats | Add-Member NoteProperty receiver_peak_memory_bytes $peak
    $stats | Add-Member NoteProperty sha256 $actual
    $stats | Export-Csv -LiteralPath (Join-Path $run 'result.csv') -NoTypeInformation
    Write-Output "PASS SHA256=$actual verified_goodput=$($stats.verified_goodput_bytes_per_second) B/s"
    Write-Output "Results: $run"
} finally {
    if (-not $receiver.HasExited) { Stop-Process -Id $receiver.Id }
}
