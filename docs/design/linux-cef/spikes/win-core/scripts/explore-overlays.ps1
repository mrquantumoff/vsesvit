# Item 7: overlays above the page.
# (a) a WindowKind::PopUp GPUI window: style, z-order against another toplevel when ours
#     deactivates, and the owner + no-TOPMOST fix;
# (b) in-window GPUI overlay with and without a SetWindowRgn cut-out of the host:
#     what is painted (PrintWindow probe) and which HWND a mouse press would hit.
param([string]$Pump = 'ext')
. "$PSScriptRoot\lib.ps1"
Start-PageServer
$null = Start-Spike -Label "overlay-$Pump" -Vars @{ SPIKE_PUMP = $Pump }
$null = Wait-Log 'load_end id=1' 10
Start-Sleep 1
# The menu rect is (content right - 272, content top - 6), 260x220 logical px.
$c = (Get-LogLines | Select-String 'win: host 0x\w+ -> (\d+)x(\d+)\+(\d+)\+(\d+) \(scale ([\d.]+)\)' | Select-Object -Last 1).Matches[0].Groups
$s = [double]$c[5].Value; $cw = [double]$c[1].Value / $s; $cy = [double]$c[4].Value / $s
$mx = [int]($cw - 272 + 130); $my = [int]($cy - 6 + 110)
$px = [int]($cw / 4); $py = [int]($cy + 300)
"menu centre ($mx,$my), page point ($px,$py)"
foreach ($mode in 'off', 'plain', 'region') {
    Mark
    ctl "menu $mode" 0.8
    ctl "probe $mx $my" 0.3
    ctl "hittest $mx $my" 0.2
    ctl "probe $px $py" 0.3
    ctl "hittest $px $py" 0.2
    ctl "shot $script:Run\menu-$mode.png" 0.5
    "--- menu $mode"
    Since | Select-String 'probe \(|hittest|win: region|menu mode' | ForEach-Object { '  ' + $_.Line }
}
Mark
ctl 'menu popup' 1
ctl "shotpopup $script:Run\popup.png" 0.5
ctl "shot $script:Run\menu-popup-main.png" 0.5
ctl zorder 0.2
ctl other 0.8
ctl zorder 0.3
ctl state 0.3
"--- popup, then another toplevel activated"
Since | Select-String 'popup window|shotpopup|zorder|other:|window activation|state:' | ForEach-Object { '  ' + ($_.Line -replace ' content=.*', '') }
Mark
ctl otherclose 0.5
ctl popupfix 0.3
ctl zorder 0.2
ctl other 0.8
ctl zorder 0.3
"--- after popupfix (owner = main, TOPMOST cleared)"
Since | Select-String 'popup: |zorder|other:' | ForEach-Object { '  ' + $_.Line }
ctl otherclose 0.3
ctl 'menu off' 0.3
ctl quit 0
$null = $script:Proc.WaitForExit(15000)
"exit=$($script:Proc.ExitCode) left=$(@(Cef-Processes).Count)"
Stop-Spike
