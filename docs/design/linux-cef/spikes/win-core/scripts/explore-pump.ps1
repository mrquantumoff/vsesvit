# Item 2: external pump on GPUI's thread vs CEF's multi_threaded_message_loop.
# Idle CPU of every spike process over 15 s with one page loaded, then load timings.
param([int]$Seconds = 15, [int]$Runs = 2)
. "$PSScriptRoot\lib.ps1"
Start-PageServer
$variants = [ordered]@{
    'ext'         = @{ SPIKE_PUMP = 'ext' }
    'ext-noidle'  = @{ SPIKE_PUMP = 'ext'; SPIKE_PUMP_IDLE_MS = '0' }
    'mt'          = @{ SPIKE_PUMP = 'mt' }
}
foreach ($name in $variants.Keys) {
    for ($r = 1; $r -le $Runs; $r++) {
        $null = Start-Spike -Label "pump-$name-$r" -Vars $variants[$name]
        if (-not (Wait-Log 'load_end id=1' 10)) { "no load"; continue }
        Start-Sleep 3
        $n = @(Cef-Processes).Count
        $c0 = Get-SpikeCpu; $t0 = Get-Date
        Start-Sleep $Seconds
        $c1 = Get-SpikeCpu; $dt = ((Get-Date) - $t0).TotalSeconds
        $pct = 100 * ($c1 - $c0) / $dt
        Mark
        ctl stats 0.3
        $stats = Last-Match 'pump: schedule'
        $loads = @()
        foreach ($u in 'page2.html', 'page1.html', 'page2.html', 'page1.html') {
            Mark
            ctl "nav http://127.0.0.1:8765/$u" 0
            if (Wait-Log 'load_end id=1' 5 -SinceMark) {
                $ts = Since | Select-String 'ctl> nav|load_end id=1' | ForEach-Object { [double]($_.Line -replace '^\[\s*([\d.]+)\].*', '$1') }
                $loads += [int](($ts[-1] - $ts[0]) * 1000)
            }
        }
        $first = Get-LogLines | Select-String 'create_browser|load_end id=1 ' | Select-Object -First 2 | ForEach-Object { [double]($_.Line -replace '^\[\s*([\d.]+)\].*', '$1') }
        $firstLoad = if ($first.Count -eq 2) { [int](($first[1] - $first[0]) * 1000) } else { -1 }
        "{0,-11} run{1} processes={2} idle_cpu={3:N2}% of one core over {4:N1}s first_load={5}ms nav_ms=[{6}] {7}" -f $name, $r, $n, $pct, $dt, $firstLoad, ($loads -join ','), $stats
        ctl quit 0
        $null = $script:Proc.WaitForExit(10000)
        Stop-Spike -KeepServer
    }
}
Stop-Spike
