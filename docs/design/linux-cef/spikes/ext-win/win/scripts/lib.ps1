# Helpers for the WX.0 Windows scripts. Dot-source this file.
# Every spike runs on the private desktop "vsesvit-extwin" through deskrun.exe; the spike
# itself refuses any other desktop (platform::guard). There is no switch to run elsewhere.
# Input goes only to the spike's own windows (posted messages), CEF host APIs or JS.
$ErrorActionPreference = 'Stop'
$script:SpikeDir = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$script:Scratch = 'C:\Users\mrqua\git\vsesvit\.audit\scratch\ext-win'
$script:Target = "$script:Scratch\target\debug"
$script:Bundle = "$script:Scratch\bundle"
$script:Fx = "$script:Scratch\fx"
$script:Desk = 'vsesvit-extwin'
$script:Ids = Get-Content "$script:Fx\ids.json" -Raw | ConvertFrom-Json

function Get-BuildEnv {
    $vsp = 'C:\Program Files\Microsoft Visual Studio\18\Community'
    $env:CMAKE = "$vsp\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe"
    if ($env:PATH -notlike "*CMake\Ninja*") { $env:PATH = "$vsp\Common7\IDE\CommonExtensions\Microsoft\CMake\Ninja;$vsp\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin;$env:PATH" }
    $env:CEF_PATH = 'C:\Users\mrqua\git\vsesvit\.audit\scratch\win-spike\cef'
    $env:CARGO_TARGET_DIR = "$script:Scratch\target"
}

function PortCtl($p) { if ($p -eq 'A') { 47811 } else { 47812 } }
function PortRep($p) { if ($p -eq 'A') { 47821 } else { 47822 } }
function RunDir($p, $label) { "$script:Scratch\runs\$label\$p" }
function RootDir($p, $label) { "$(RunDir $p $label)\root" }

# Processes started from the scratch directory (spike, CEF helpers, deskrun).
function Spike-Processes { Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.StartsWith($script:Scratch, [StringComparison]::OrdinalIgnoreCase) } }

# One control line; returns the reply line.
function c {
    param([Parameter(Mandatory)][string]$P, [Parameter(Mandatory)][string]$Line, [int]$Timeout = 130)
    $cl = New-Object System.Net.Sockets.TcpClient
    $cl.Connect('127.0.0.1', (PortCtl $P))
    $cl.ReceiveTimeout = $Timeout * 1000
    $s = $cl.GetStream()
    $w = New-Object System.IO.StreamWriter($s); $w.NewLine = "`n"; $w.AutoFlush = $true
    $r = New-Object System.IO.StreamReader($s)
    $w.WriteLine($Line)
    $ans = $r.ReadLine()
    $cl.Close()
    $ans
}

function Try-Ping($p) { try { (c -P $p -Line 'ping' -Timeout 2) -eq 'ok' } catch { $false } }

# Start profile A or B. The CEF root persists per label, so the same label again is a restart.
function Start-Profile {
    param([Parameter(Mandatory)][string]$P, [Parameter(Mandatory)][string]$Label, [hashtable]$Vars = @{}, [switch]$Plain, [string]$Desktop = $script:Desk,
          [string]$Urls = 'http://site.vsesvit.test/index.html')
    $run = RunDir $P $Label
    New-Item -ItemType Directory -Force -Path $run | Out-Null
    if (-not (Test-Path "$run\site")) { Copy-Item "$script:Fx\site" "$run\site" -Recurse }
    $exe = if ($Plain) { "$script:Target\vsesvit-extwin.exe" } else { "$script:Bundle\extwin.exe" }
    Add-Content "$run\spike.log" "=== start $(Get-Date -Format HH:mm:ss) bundle=$(-not $Plain) $($Vars.Keys -join ',')"
    $all = @{
        SPIKE_LOG = "$run\spike.log"; SPIKE_ROOT = "$run\root"; SPIKE_CTL_PORT = "$(PortCtl $P)"; SPIKE_REPORT_PORT = "$(PortRep $P)"
        SPIKE_SITE = "$run\site"; SPIKE_REPORTS = "$run\reports.jsonl"; SPIKE_URLS = $Urls; SPIKE_POS = '-4000,-4000'
    }
    foreach ($k in $Vars.Keys) { $all[$k] = $Vars[$k] }
    $old = @{}
    foreach ($k in $all.Keys) { $old[$k] = [Environment]::GetEnvironmentVariable($k); [Environment]::SetEnvironmentVariable($k, $all[$k]) }
    $io = @{ WorkingDirectory = $run; PassThru = $true; WindowStyle = 'Hidden'; RedirectStandardError = "$run\stderr.txt"; RedirectStandardOutput = "$run\stdout.txt" }
    $proc = Start-Process -FilePath "$script:Target\deskrun.exe" -ArgumentList "$Desktop `"$exe`"" @io
    foreach ($k in $old.Keys) { [Environment]::SetEnvironmentVariable($k, $old[$k]) }
    Set-Content "$run\deskrun.pid" $proc.Id
    $deadline = (Get-Date).AddSeconds(30)
    while ((Get-Date) -lt $deadline) {
        if ($proc.HasExited) { return "EXITED code=$($proc.ExitCode)" }
        if (Try-Ping $P) { return "profile $P up (deskrun pid $($proc.Id)) log $run\spike.log" }
        Start-Sleep -Milliseconds 200
    }
    "TIMEOUT starting $P"
}

# Quit cleanly, wait up to 25 s for deskrun (it waits for the browser), then kill leftovers of this profile.
function Stop-Profile {
    param([Parameter(Mandatory)][string]$P, [Parameter(Mandatory)][string]$Label)
    $run = RunDir $P $Label
    $dpid = [int](Get-Content "$run\deskrun.pid" -ErrorAction SilentlyContinue)
    try { $null = c -P $P -Line 'quit' -Timeout 5 } catch {}
    $d = Get-Process -Id $dpid -ErrorAction SilentlyContinue
    if ($d) { if (-not $d.WaitForExit(25000)) { "profile $P did not exit; killing its tree"; Kill-Tree $dpid } }
    (Get-Content "$run\spike.log" | Select-String 'cef shutdown in|exiting|GUARD' | Select-Object -Last 1).Line
}

function Kill-Tree($id) {
    Get-CimInstance Win32_Process -Filter "ParentProcessId=$id" | ForEach-Object { Kill-Tree $_.ProcessId }
    Stop-Process -Id $id -Force -ErrorAction SilentlyContinue
}

function Kill-All { Spike-Processes | Stop-Process -Force -ErrorAction SilentlyContinue; Start-Sleep -Milliseconds 300; "spike processes left: $((Spike-Processes | Measure-Object).Count)" }

function Log-Lines($p, $label) { Get-Content "$(RunDir $p $label)\spike.log" }
function Reports($p, $label) { if (Test-Path "$(RunDir $p $label)\reports.jsonl") { Get-Content "$(RunDir $p $label)\reports.jsonl" } }
function Wait-Log { param($P, $Label, $Pattern, [double]$Timeout = 10)
    $deadline = (Get-Date).AddSeconds($Timeout)
    do { if (Select-String -Path "$(RunDir $P $Label)\spike.log" -Pattern $Pattern -Quiet) { return $true }; Start-Sleep -Milliseconds 200 } while ((Get-Date) -lt $deadline)
    $false
}
function Prefs($p, $label) { & python3 "$script:SpikeDir\tools\prefs.py" "$(RootDir $p $label)\Default" "$script:Fx\ids.json" }
function Say($s) { "## $s" }

# The helper's bridge page in a hidden Alloy browser, and calls into it.
function Bridge($p) { c -P $p -Line "@20 hidden bridge alloy chrome-extension://$($script:Ids.H)/bridge.html" }
function Call($p, $json) { c -P $p -Line "@20 ev bridge vsesvitBridge.call($json)" }

# Archive NAME-VERSION.crx in the profile (as core would) and return its path.
function Archive($p, $label, $name, $ver) {
    $a = "$(RunDir $p $label)\archive"; New-Item -ItemType Directory -Force -Path $a | Out-Null
    Copy-Item "$script:Fx\crx\$name-$ver.crx" "$a\$name-$ver.crx" -Force
    "$a\$name-$ver.crx"
}
