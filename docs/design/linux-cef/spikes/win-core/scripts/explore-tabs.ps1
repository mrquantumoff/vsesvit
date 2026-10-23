# Item 4: three tabs, switching with show/hide + was_hidden, closing, and 30 open/close
# cycles checked for leaked processes, handles, GDI/USER objects and HWNDs.
param([string]$Pump = 'ext', [int]$Cycles = 30)
. "$PSScriptRoot\lib.ps1"
Start-PageServer
$null = Start-Spike -Label "tabs-$Pump" -Vars @{ SPIKE_PUMP = $Pump }
$null = Wait-Log 'load_end id=1' 10
ctl 'new http://127.0.0.1:8765/page2.html' 0; $null = Wait-Log 'load_end id=2' 10
ctl 'new http://127.0.0.1:8765/page1.html' 0; $null = Wait-Log 'load_end id=3' 10
Start-Sleep 1
foreach ($i in 0, 1, 2, 0) {
    Mark
    ctl "switch $i" 0.8
    ctl state 0.3
    "--- switch $i"
    Since | Select-String 'activated tab|visibility|^\s+tab \d+:' | ForEach-Object { '  ' + ($_.Line -replace 'rect=Some\(PRect \{[^}]*\}\) ', '' -replace ' url=\S+ favicon=.*', '') }
}
Mark
ctl 'close 1' 1.5
ctl state 0.3
"--- close 1"
Since | Select-String 'close_browser|on_before_close|tabs left|activated tab|^\s+tab \d+:' | ForEach-Object { '  ' + ($_.Line -replace 'rect=Some\(PRect \{[^}]*\}\) ', '' -replace ' url=\S+ favicon=.*', '') }

Start-Sleep 2
Mark; ctl res 0.3; $r0 = Last-Match 'res:'; $p0 = @(Cef-Processes).Count
$ok = 0
for ($c = 1; $c -le $Cycles; $c++) {
    Mark
    ctl 'new http://127.0.0.1:8765/page2.html' 0
    if (-not (Wait-Log 'load_end id=\d+ status=200' 10 -SinceMark)) { "cycle ${c}: no load"; break }
    $line = Last-Match 'tab \d+ <- browser id=(\d+)'
    $idx = [int]($line -replace '.*gpui: tab (\d+) <-.*', '$1')
    ctl "close $idx" 0
    if (Wait-Log 'tabs left 2' 10 -SinceMark) { $ok++ } else { "cycle ${c}: no close" }
}
Start-Sleep 4
Mark; ctl res 0.3; $r1 = Last-Match 'res:'; $p1 = @(Cef-Processes).Count
"--- $ok/$Cycles cycles"
"  before: processes=$p0 $r0"
"  after:  processes=$p1 $r1"
Mark; ctl quit 0
$null = $script:Proc.WaitForExit(15000)
"  exit=$($script:Proc.ExitCode) left=$(@(Cef-Processes).Count)"
Stop-Spike
