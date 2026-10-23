# Item 5: keyboard focus between the page and GPUI, keys posted through the queue.
# Tab-out of the page to the omnibox, Ctrl+L from the chrome, click back into the page,
# then the user-desktop policy (SPIKE_STRICT=1): nothing may activate itself.
param([string]$Pump = 'ext')
. "$PSScriptRoot\lib.ps1"
Start-PageServer
$null = Start-Spike -Label "focus-$Pump" -Vars @{ SPIKE_PUMP = $Pump }
$null = Wait-Log 'load_end id=1' 10
Start-Sleep 0.5
Mark
$n = 0
for ($i = 1; $i -le 30; $i++) {
    ctl 'keypost tab' 0.15
    if (Wait-Log 'take_focus' 0.2 -SinceMark) { $n = $i; break }
}
ctl state 0.3
"--- Tab x$n until on_take_focus"
Since | Select-String 'take_focus|key focus|focus_omnibox|address input|state:' | ForEach-Object { '  ' + ($_.Line -replace ' content=.*', '') }
Mark
ctl 'clickel text' 0.8
ctl 'keypost l ctrl to=page' 0.6
ctl 'keypost tab' 0.4
ctl state 0.3
"--- click page, Ctrl+L from the page, then Tab inside GPUI"
Since | Select-String 'got_focus|key focus|action|address input|state:|keydown' | ForEach-Object { '  ' + ($_.Line -replace ' content=.*', '') }
Mark
ctl 'page' 0.5
ctl 'omnibox' 0.5
ctl 'keypost l ctrl to=chrome' 0.5
ctl state 0.3
"--- Ctrl+L with the chrome focused (GPUI key binding)"
Since | Select-String 'key focus|focus_omnibox|address input|state:|pre_key' | ForEach-Object { '  ' + ($_.Line -replace ' content=.*', '') }
ctl quit 0
$null = $script:Proc.WaitForExit(15000)
Stop-Spike -KeepServer

$null = Start-Spike -Label "focus-strict-$Pump" -Vars @{ SPIKE_PUMP = $Pump; SPIKE_STRICT = '1' }
$null = Wait-Log 'load_end id=1' 10
Start-Sleep 3
ctl 'nav http://127.0.0.1:8765/page2.html' 1.5
ctl 'new http://127.0.0.1:8765/page1.html' 2
ctl state 0.3
ctl quit 0
$null = $script:Proc.WaitForExit(15000)
"--- strict (user-desktop policy on the hidden desktop)"
Get-LogLines | Select-String 'may_activate|activation|refused|SetFocus|got_focus|set_focus|state:' | ForEach-Object { '  ' + ($_.Line -replace ' content=.*', '') }
Stop-Spike
