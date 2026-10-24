# Shared helpers for the spike scripts. Dot-source this file.
# Everything runs against the spike's own windows only: no SendInput, no foreground changes,
# no screen capture. The spike runs only on a separate desktop (deskrun.exe) that is never
# switched to, so activation, fullscreen and Chrome's own windows stay invisible to the user.
# An empty -Desktop, which once ran it on the user's desktop, is refused (design section 7.3).
$ErrorActionPreference = 'Stop'
$script:SpikeDir = Split-Path -Parent $PSScriptRoot
$script:Scratch = 'C:\Users\mrqua\git\vsesvit\.audit\scratch\win-spike'
$script:Target = "$script:Scratch\target\debug"
$script:CtlPort = 47811
$script:PagePort = 8765
$script:Log = $null
$script:Desk = 'vsesvit-spike'

function Get-SpikeEnv {
    $vsp = 'C:\Program Files\Microsoft Visual Studio\18\Community'
    $env:CMAKE = "$vsp\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe"
    if ($env:PATH -notlike "*CMake\Ninja*") {
        $env:PATH = "$vsp\Common7\IDE\CommonExtensions\Microsoft\CMake\Ninja;$vsp\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin;$env:PATH"
    }
    $env:CEF_PATH = "$script:Scratch\cef"
    $env:CARGO_TARGET_DIR = "$script:Scratch\target"
}

# Only processes started from the spike's scratch directory.
function Spike-Processes {
    Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.StartsWith($script:Scratch, [StringComparison]::OrdinalIgnoreCase) }
}

# Browser plus CEF helpers (not the page server or the launcher).
function Cef-Processes { Spike-Processes | Where-Object { $_.Name -match '^(vsesvit-win-spike|winspike)' } }

function Stop-Spike {
    param([switch]$KeepServer)
    Spike-Processes | Where-Object { -not ($KeepServer -and $_.Name -eq 'serve') } | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 300
}

function Start-PageServer {
    $busy = Get-NetTCPConnection -LocalPort $script:PagePort -State Listen -ErrorAction SilentlyContinue
    if (-not $busy) {
        Start-Process -FilePath "$script:Target\serve.exe" -ArgumentList "`"$script:SpikeDir\testpages`" $script:PagePort" -WindowStyle Hidden | Out-Null
        Start-Sleep -Milliseconds 300
    }
}

# Start the spike on the private desktop $Desktop. $Vars: extra environment (hashtable).
# Returns the launched deskrun process; sets $script:Log and $script:Run.
function Start-Spike {
    param([string]$Label = 'run', [hashtable]$Vars = @{}, [string]$Urls = 'http://127.0.0.1:8765/page1.html', [string]$Desktop = $script:Desk, [string]$Exe = "$script:Target\vsesvit-win-spike.exe")
    if (-not $Desktop) { throw "Start-Spike: refusing an empty -Desktop; the spike never runs on the user's desktop" }
    Stop-Spike -KeepServer   # a failed script may have left one holding the ctl port
    $run = "$script:Scratch\runs\$Label"
    if (Test-Path $run) { Remove-Item -Recurse -Force $run }
    New-Item -ItemType Directory -Force -Path $run | Out-Null
    $script:Log = "$run\spike.log"
    $script:Run = $run
    $old = @{}
    # PowerShell names are case-insensitive: $all, not $vars (that is $Vars).
    $all = @{
        SPIKE_LOG      = $script:Log
        SPIKE_ROOT     = "$run\cef-root"
        SPIKE_CTL_PORT = "$script:CtlPort"
        SPIKE_URLS     = $Urls
        SPIKE_POS      = '-4000,-4000'
    }
    foreach ($k in $Vars.Keys) { $all[$k] = $Vars[$k] }
    foreach ($k in $all.Keys) { $old[$k] = [Environment]::GetEnvironmentVariable($k); [Environment]::SetEnvironmentVariable($k, $all[$k]) }
    $io = @{ WorkingDirectory = $run; PassThru = $true; WindowStyle = 'Hidden'; RedirectStandardError = "$run\stderr.txt"; RedirectStandardOutput = "$run\stdout.txt" }
    $p = Start-Process -FilePath "$script:Target\deskrun.exe" -ArgumentList "$Desktop `"$exe`"" @io
    foreach ($k in $old.Keys) { [Environment]::SetEnvironmentVariable($k, $old[$k]) }
    $script:Proc = $p
    if (-not (Wait-Log 'ctl: listening' 20)) { throw "spike did not start (see $script:Log)" }
    return $p
}

# Send one control line and wait for the ack. $Settle: seconds to wait afterwards.
function ctl {
    param([Parameter(Mandatory)][string]$Line, [double]$Settle = 0.3)
    $c = New-Object System.Net.Sockets.TcpClient
    $c.Connect('127.0.0.1', $script:CtlPort)
    $s = $c.GetStream()
    $w = New-Object System.IO.StreamWriter($s); $w.NewLine = "`n"; $w.AutoFlush = $true
    $r = New-Object System.IO.StreamReader($s)
    $w.WriteLine($Line)
    $null = $r.ReadLine()
    $c.Close()
    if ($Settle -gt 0) { Start-Sleep -Milliseconds ([int]($Settle * 1000)) }
}

function Get-LogLines { if (Test-Path $script:Log) { @(Get-Content $script:Log) } else { @() } }

# Remember the current log length; Since returns the lines added after the last Mark.
function Mark { $script:MarkLen = @(Get-LogLines).Count }
function Since { $all = @(Get-LogLines); if ($all.Count -gt $script:MarkLen) { $all[$script:MarkLen..($all.Count - 1)] } else { @() } }

# Wait until a regex appears in the log (after the mark when -SinceMark). Returns $true on success.
function Wait-Log {
    param([Parameter(Mandatory)][string]$Pattern, [double]$Timeout = 5, [switch]$SinceMark)
    $deadline = (Get-Date).AddSeconds($Timeout)
    do {
        $lines = if ($SinceMark) { Since } else { Get-LogLines }
        if (@($lines | Select-String -Pattern $Pattern -Quiet) -contains $true) { return $true }
        Start-Sleep -Milliseconds 100
    } while ((Get-Date) -lt $deadline)
    return $false
}

# Last log line (after the mark) matching a regex, or $null.
function Last-Match {
    param([Parameter(Mandatory)][string]$Pattern)
    $m = @(Since | Select-String -Pattern $Pattern)
    if ($m.Count) { $m[-1].Line } else { $null }
}

# CPU time (seconds) summed over the browser and its CEF helpers.
function Get-SpikeCpu { ($(Cef-Processes | ForEach-Object { $_.TotalProcessorTime.TotalSeconds }) | Measure-Object -Sum).Sum }

# Simple PASS/FAIL accounting for the test scripts.
$script:Pass = 0; $script:Fail = 0; $script:Failed = @()
function Check {
    param([string]$Name, [bool]$Ok, [string]$Detail = '')
    if ($Ok) { $script:Pass++; "PASS $Name $Detail" } else { $script:Fail++; $script:Failed += $Name; "FAIL $Name $Detail" }
}
function Summary { "== $script:Pass passed, $script:Fail failed"; if ($script:Failed.Count) { "failed: $($script:Failed -join ', ')" } }

# Fails the run when the spike ever reported disturbing the user's desktop.
function Check-Safety { Check 'safety: no foreground or on-screen window' (-not (Wait-Log 'SAFETY:' 0)) }
