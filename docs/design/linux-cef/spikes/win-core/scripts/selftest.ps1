# End-to-end self-test of the Windows spike. Every run is on the private desktop.
# Prints PASS/FAIL per check and "== N passed, M failed". Needs a built target dir and,
# for the sandbox check, scripts\bundle-sandbox.ps1 run once.
. "$PSScriptRoot\lib.ps1"
Start-PageServer
function Num([string]$Line, [string]$Pattern) { if ($Line -match $Pattern) { [double]$Matches[1] } else { -1 } }

# ---- A: external pump, shell dialogs, both key hooks
$null = Start-Spike -Label 'st-ext' -Vars @{ SPIKE_DIALOGS = 'shell' }
Check 'A1 page loads (load_end 200)' (Wait-Log 'load_end id=1 status=200' 15)
Check 'A2 private desktop, may_activate' (Wait-Log 'may_activate=true' 0)
Start-Sleep 1
Mark; ctl content 0.6
Check 'A3 page visible through GPUI (content > 0.9)' ((Num (Last-Match 'non_background') 'non_background=([\d.]+)') -gt 0.9)
Mark; ctl dpi 0.6
$scale = Num (Last-Match 'gpui scale') 'gpui scale=([\d.]+)'
Check 'A4 devicePixelRatio equals GPUI scale' ($scale -gt 0 -and (Num (Last-Match 'page: dpr=') 'dpr=([\d.]+)') -eq $scale) "scale=$scale"
$t = (Get-LogLines | Select-String 'tab 0 <- browser' | Select-Object -First 1).Line
Check 'A5 CEF child is ours and on the GPUI thread' ($t -match '\(ours=true\)' -and $t -match 'tid=(\d+) main_tid=(\d+)' -and $Matches[1] -eq $Matches[2])
Mark; ctl 'keypost x to=page' 0.6
Check 'A6 posted key reaches the page' (Wait-Log 'page: keydown key=x' 1 -SinceMark)
Mark; ctl 'keypost l ctrl to=page' 0.6; ctl state 0.3
$st = Last-Match 'state:'
Check 'A7 Ctrl+L in page -> omnibox, window stays active' ($st -match 'omnibox_focused=true' -and $st -match 'gpui_active=true')
Mark; ctl 'typepost abc' 0.5; ctl state 0.3
Check 'A8 typing lands in the omnibox' ((Last-Match 'state:') -match 'address="abc"')
Mark; ctl 'clickel text' 0.8; ctl state 0.3
$st = Last-Match 'state:'
Check 'A9 click page -> page focus, window active' ($st -match 'key_focus=Page' -and $st -match 'gpui_active=true' -and $st -match 'page_focused=true')
Mark
for ($i = 0; $i -lt 30 -and -not (Wait-Log 'take_focus' 0.1 -SinceMark); $i++) { ctl 'keypost tab' 0.15 }
Check 'A10 Tab out of the page -> omnibox' (Wait-Log 'take_focus' 1 -SinceMark)
Mark; ctl 'resize 1000 650' 1.2
$w = [int](1000 * $scale)
Check 'A11 child follows window resize' (Wait-Log "win: child 0x\w+ -> $w`x" 1 -SinceMark) "expect width $w"
Mark; ctl 'menu region' 0.8; ctl 'hittest 900 160' 0.3; ctl 'menu plain' 0.8; ctl 'hittest 900 160' 0.3; ctl 'menu off' 0.3
$h = @(Since | Select-String 'hittest \(')
Check 'A12 region cut-out routes overlay clicks to GPUI' ($h.Count -eq 2 -and $h[0].Line -match 'window_from_point=0x\w+ "Zed::Window"' -and $h[1].Line -match 'window_from_point=0x\w+ "Chrome_')
Mark; ctl 'menu popup' 1; ctl other 0.8; ctl zorder 0.3
$z1 = Last-Match 'zorder:'
ctl otherclose 0.4; ctl popupfix 0.3; ctl other 0.8; ctl zorder 0.3
$z2 = Last-Match 'zorder:'
ctl otherclose 0.3; ctl 'menu off' 0.3
$above = { param($z) if ($z -match 'popup=Some\((\d+)\).*other=Some\((\d+)\)') { [int]$Matches[1] -lt [int]$Matches[2] } else { $null } }
Check 'A13 PopUp TOPMOST floats over other apps; owner+no-topmost fixes it' ((& $above $z1) -eq $true -and (& $above $z2) -eq $false)
Mark; ctl 'shotn 5' 1
$sn = Last-Match 'shotn:'
Check 'A14 in-process screenshot (5x PrintWindow < 100 ms avg)' ($sn -match 'ok=5' -and (Num $sn 'avg=([\d.]+)ms') -lt 100) $sn
Mark; ctl 'clickel open' 1.5
Check 'A15 window.open -> child tab in our window' (Wait-Log 'popup=true opener=1 .*\(ours=true\)' 2 -SinceMark)
ctl 'switch 0' 0.5
Mark; ctl 'clickel alert' 1; ctl 'clickel file' 1; ctl 'clickel notif' 1
Check 'A16 JS dialog, file chooser, permission reach the shell' ((Wait-Log 'page: alert returned' 1 -SinceMark) -and (Wait-Log 'page: file -> picked.txt' 1 -SinceMark) -and (Wait-Log 'page: notification -> granted' 1 -SinceMark))
Mark; ctl 'keypost t ctrl to=page' 1; ctl 'keypost f12 to=page' 1; ctl tops 0.3
# F12 lands on the new about:blank tab while the omnibox has focus; Chrome then skips
# on_pre_key_event and fires IDC_DEV_TOOLS_TOGGLE, which on_chrome_command catches.
Check 'A17 Ctrl+T and F12 go to the shell, no Chrome-native window' ((Wait-Log 'action NewTab from page id=\d+ via on_pre_key_event' 0 -SinceMark) -and (Wait-Log 'action DevTools from page id=\d+ via (on_pre_key_event|on_chrome_command)' 0 -SinceMark) -and -not (Wait-Log '^0x\w+ "Chrome_WidgetWin_1" "[^"]+" \S+ visible' 0 -SinceMark))
Mark; ctl state 0.3
$tabs = @(Since | Select-String '^\s+tab \d+: ')
$vis = @($tabs | Where-Object { $_.Line -match 'visible=true' })
Check 'A18 several tabs, exactly one visible' ($tabs.Count -ge 3 -and $vis.Count -eq 1) "tabs=$($tabs.Count)"
function Cycle {
    Mark; ctl 'new http://127.0.0.1:8765/page2.html' 0
    $null = Wait-Log 'load_end id=\d+ status=200' 10 -SinceMark
    $idx = [int]((Last-Match 'gpui: tab (\d+) <-') -replace '.*gpui: tab (\d+) <-.*', '$1')
    ctl "close $idx" 0; $null = Wait-Log 'tabs left' 10 -SinceMark
}
function Res([string]$l) { if ($l -match 'handles=(\d+) gdi=(\d+) user=(\d+) toplevels=(\d+) windows_under_gpui=(\d+)') { [int[]]$Matches[1..5] } else { @() } }
Cycle   # warm-up: the first tab after startup allocates for good
Start-Sleep 2
Mark; ctl res 0.3; $r0 = (Last-Match 'res:') -replace '^\[[^\]]*\]\s*', ''; $p0 = @(Cef-Processes).Count
for ($c = 0; $c -lt 10; $c++) { Cycle }
Start-Sleep 4
Mark; ctl res 0.3; $r1 = (Last-Match 'res:') -replace '^\[[^\]]*\]\s*', ''; $p1 = @(Cef-Processes).Count
$a = Res $r0; $b = Res $r1
Check 'A19 10 open/close cycles leak nothing (handles +-20, GDI/USER +-2, HWNDs and processes equal)' ($a.Count -eq 5 -and $b.Count -eq 5 -and [Math]::Abs($b[0] - $a[0]) -le 20 -and [Math]::Abs($b[1] - $a[1]) -le 2 -and [Math]::Abs($b[2] - $a[2]) -le 2 -and $b[3] -eq $a[3] -and $b[4] -eq $a[4] -and $p0 -eq $p1) "before: $r0 p=$p0 after: $r1 p=$p1"
Check-Safety
Mark; ctl closewin 0
$ok = $script:Proc.WaitForExit(15000); Start-Sleep 0.5
Check 'A20 window close: exit 0, no processes left' ($ok -and $script:Proc.ExitCode -eq 0 -and @(Cef-Processes).Count -eq 0)

# ---- B: multi-threaded message loop
$null = Start-Spike -Label 'st-mt' -Vars @{ SPIKE_PUMP = 'mt' }
Check 'B1 mt: page loads on CEF''s own UI thread' ((Wait-Log 'load_end id=1 status=200' 15) -and ((Get-LogLines | Select-String 'tab 0 <- browser').Line -match 'tid=(\d+) main_tid=(\d+)') -and $Matches[1] -ne $Matches[2])
Start-Sleep 0.5
Mark; ctl 'keypost x to=page' 0.6
Check 'B2 mt: posted key reaches the page' (Wait-Log 'page: keydown key=x' 1 -SinceMark)
ctl quit 0
$ok = $script:Proc.WaitForExit(15000); Start-Sleep 0.5
Check 'B3 mt: quit exits 0, no processes left' ($ok -and $script:Proc.ExitCode -eq 0 -and @(Cef-Processes).Count -eq 0)

# ---- C: negative control, GPUI's message loop unpatched
$null = Start-Spike -Label 'st-unpatched' -Vars @{ GPUI_SPIKE_UNPATCHED = '1' }
$null = Wait-Log 'load_end id=1 status=200' 15; Start-Sleep 0.5
Mark; ctl 'keypost x to=page' 0.8; ctl 'key y to=page' 0.6
Check 'C1 unpatched GPUI drops posted keys to the page (SendMessage ones arrive)' (-not (Wait-Log 'page: keydown key=x' 0 -SinceMark) -and (Wait-Log 'page: keydown key=y' 0 -SinceMark))
ctl quit 0; $null = $script:Proc.WaitForExit(15000)

# ---- D: user-desktop policy: nothing activates itself
$null = Start-Spike -Label 'st-strict' -Vars @{ SPIKE_STRICT = '1' }
$null = Wait-Log 'load_end id=1 status=200' 15; Start-Sleep 1.5
Mark; ctl state 0.3
Check 'D1 strict policy: window never activates, focus refused' ((Last-Match 'state:') -match 'gpui_active=false' -and (Wait-Log 'refused SetFocus' 0) -and -not (Wait-Log 'activation changed active=true' 0))
ctl quit 0; $null = $script:Proc.WaitForExit(15000)

# ---- E: sandbox through bootstrap.exe
$bundle = "$script:Scratch\bundle\winspike.exe"
if (Test-Path $bundle) {
    $null = Start-Spike -Label 'st-sandbox' -Exe $bundle -Urls 'chrome://sandbox'
    Start-Sleep 5
    Mark; ctl "js console.log('SANDBOX '+document.body.innerText.replace(/\n+/g,' | '))" 1
    Check 'E1 sandbox: renderers locked down at Untrusted integrity' ((Last-Match 'SANDBOX') -match 'Renderer\s+Renderer\s+Lockdown\s+S-1-16-0 Untrusted')
    ctl quit 0
    $ok = $script:Proc.WaitForExit(15000); Start-Sleep 0.5
    Check 'E2 sandbox: exit 0, no processes left' ($ok -and $script:Proc.ExitCode -eq 0 -and @(Cef-Processes).Count -eq 0)
} else { Check 'E sandbox bundle present (run scripts\bundle-sandbox.ps1)' $false }

Check-Safety
Stop-Spike
Summary
