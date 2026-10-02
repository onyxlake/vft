param(
    [long[]]$Sizes = @(1024, 1048576, 104857600),
    [ValidateSet('robust','balanced')][string]$Profile = 'balanced',
    [int]$Fps = 30,
    [int]$CellSize = 4,
    [int]$TimeoutSeconds = 3600,
    [int]$JoinDelaySeconds = 0,
    [switch]$AutoLocate,
    [switch]$ManualRect,
    [switch]$MoveSender,
    [switch]$SkipBuild
)
$ErrorActionPreference = 'Stop'
if ($MoveSender -and -not $AutoLocate) { throw '-MoveSender requires -AutoLocate' }
if ($ManualRect -and $AutoLocate) { throw '-ManualRect and -AutoLocate are mutually exclusive' }
if ($MoveSender -and -not ('VftE2EWindow' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class VftE2EWindow {
    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool SetWindowPos(IntPtr window, IntPtr after, int x, int y, int width, int height, uint flags);
}
'@
}
$project = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
if (-not $SkipBuild) {
    Push-Location $project
    try { cargo build --release --workspace; if ($LASTEXITCODE) { throw 'Build failed' } }
    finally { Pop-Location }
}
$run = Join-Path $project ('output/local-' + [DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss') + '-' + [guid]::NewGuid().ToString('N').Substring(0,8))
New-Item -ItemType Directory -Path $run | Out-Null
$sendExe = Join-Path $project 'target/release/vft-send-win.exe'
$recvExe = Join-Path $project 'target/release/vft-recv-win.exe'
$results = @()
foreach ($size in $Sizes) {
    if ($size -lt 0) { throw 'File size must be nonnegative' }
    $inputPath = Join-Path $run "$size-input.bin"
    $outputPath = Join-Path $run "$size-recovered.bin"
    $rectPath = Join-Path $run "$size-region.txt"
    $statsPath = Join-Path $run "$size-stats.csv"
    $stream = [IO.File]::Open($inputPath, [IO.FileMode]::CreateNew)
    try {
        $random = [Random]::new(20260930)
        $buffer = [byte[]]::new(65536)
        $remaining = $size
        while ($remaining -gt 0) {
            $random.NextBytes($buffer)
            $count = [int][Math]::Min($buffer.Length, $remaining)
            $stream.Write($buffer, 0, $count)
            $remaining -= $count
        }
    } finally { $stream.Dispose() }
    $sender = $null
    $receiver = $null
    $receiverCpu = 0.0
    $receiverPeak = 0L
    $moved = $false
    try {
        $sendArgs = @(('"' + $inputPath + '"'),'--no-tui','--profile',$Profile,'--fps',"$Fps",'--cell-size',"$CellSize",'--topmost','--rect-file',('"' + $rectPath + '"'))
        $sender = Start-Process -FilePath $sendExe -ArgumentList $sendArgs -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $run "$size-sender.log") -RedirectStandardError (Join-Path $run "$size-sender.err")
        $readyDeadline = [DateTime]::UtcNow.AddSeconds(15)
        while (-not (Test-Path -LiteralPath $rectPath)) {
            if ($sender.HasExited) { throw "Sender exited: $(Get-Content -LiteralPath (Join-Path $run "$size-sender.err") -Raw)" }
            if ([DateTime]::UtcNow -gt $readyDeadline) { throw 'Sender window did not paint in 15 seconds' }
            Start-Sleep -Milliseconds 100
        }
        if ($JoinDelaySeconds -gt 0) { Start-Sleep -Seconds $JoinDelaySeconds }
        $captureMode = if ($AutoLocate) { '--auto' } elseif ($ManualRect) { '--rect' } else { '--sender-window' }
        $captureArgs = @($captureMode)
        if ($ManualRect) { $captureArgs += (Get-Content -LiteralPath $rectPath -Raw).Trim() -split '\s+' }
        $recvArgs = $captureArgs + @('--no-tui','--output',('"' + $outputPath + '"'),'--fps','120','--timeout',"$TimeoutSeconds",'--stats',('"' + $statsPath + '"'),'--preview',('"' + (Join-Path $run "$size-preview.bmp") + '"'))
        $receiver = Start-Process -FilePath $recvExe -ArgumentList $recvArgs -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $run "$size-receiver.log") -RedirectStandardError (Join-Path $run "$size-receiver.err")
        $receiveStart = [DateTime]::UtcNow
        while (-not $receiver.WaitForExit(1000)) {
            if ($sender.HasExited) { throw 'Sender exited during capture' }
            $receiver.Refresh()
            $receiverCpu = $receiver.TotalProcessorTime.TotalSeconds
            $receiverPeak = [Math]::Max($receiverPeak,$receiver.PeakWorkingSet64)
            if ($MoveSender -and -not $moved -and ([DateTime]::UtcNow - $receiveStart).TotalSeconds -ge 3 -and
                (Select-String -LiteralPath (Join-Path $run "$size-receiver.log") -Pattern '^LOCATED ' -Quiet)) {
                $sender.Refresh()
                $handle = $sender.MainWindowHandle
                if ($handle -eq [IntPtr]::Zero -or -not [VftE2EWindow]::SetWindowPos($handle, [IntPtr]::Zero, 180, 120, 0, 0, 0x0015)) {
                    throw 'Could not move the sender window owned by this test'
                }
                $moved = $true
            }
        }
        if ($receiver.ExitCode -ne 0) { throw "Receiver failed: $(Get-Content -LiteralPath (Join-Path $run "$size-receiver.err") -Raw)" }
        $expected = (Get-FileHash -LiteralPath $inputPath -Algorithm SHA256).Hash
        $actual = (Get-FileHash -LiteralPath $outputPath -Algorithm SHA256).Hash
        if ($expected -ne $actual) { throw 'Output SHA-256 does not match input' }
        $sender.Refresh(); $receiver.Refresh()
        $receiverCpu = $receiver.TotalProcessorTime.TotalSeconds
        $receiverPeak = [Math]::Max($receiverPeak,$receiver.PeakWorkingSet64)
        $stats = Import-Csv -LiteralPath $statsPath
        if ($AutoLocate -and [long]$stats.location_locks -lt 1) { throw 'Auto mode did not locate a frame' }
        if ($MoveSender -and (-not $moved -or [long]$stats.location_locks -lt 2)) { throw 'Movement test did not reacquire the sender' }
        $stats | Add-Member NoteProperty sender_cpu_seconds $sender.TotalProcessorTime.TotalSeconds
        $stats | Add-Member NoteProperty sender_peak_memory_bytes $sender.PeakWorkingSet64
        $stats | Add-Member NoteProperty receiver_cpu_seconds $receiverCpu
        $peakValue = if ($receiverPeak -gt 0) { $receiverPeak } else { $null }
        $stats | Add-Member NoteProperty receiver_peak_memory_bytes $peakValue
        $stats | Add-Member NoteProperty sha256 $actual
        $stats | Add-Member NoteProperty requested_fps $Fps
        $stats | Add-Member NoteProperty cell_size $CellSize
        $stats | Add-Member NoteProperty join_delay_seconds $JoinDelaySeconds
        $stats | Add-Member NoteProperty capture_mode $captureMode
        $stats | Add-Member NoteProperty sender_moved $moved
        $stats | Add-Member NoteProperty capture_region (Get-Content -LiteralPath $rectPath -Raw)
        $results += $stats
        Write-Output "PASS size=$size SHA256=$actual goodput=$($stats.verified_goodput_bytes_per_second) B/s"
    } finally {
        # Only stop the two processes launched by this test; never stop unrelated windows.
        if ($receiver -and -not $receiver.HasExited) { Stop-Process -Id $receiver.Id }
        if ($sender -and -not $sender.HasExited) { Stop-Process -Id $sender.Id }
    }
}
$results | Export-Csv -LiteralPath (Join-Path $run 'results.csv') -NoTypeInformation
Write-Output "Results: $run"
