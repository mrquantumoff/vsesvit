# Item 10: clean shutdown from the shell action (ctl quit) and from the window close
# (WM_CLOSE posted to our own toplevel), with two tabs open, in each pump mode.
param([int]$Reps = 3)
. "$PSScriptRoot\lib.ps1"
Start-PageServer
foreach ($pump in 'ext', 'mt') {
    foreach ($how in 'quit', 'closewin') {
        for ($r = 1; $r -le $Reps; $r++) {
            $null = Start-Spike -Label "down-$pump-$how-$r" -Vars @{ SPIKE_PUMP = $pump }
            $null = Wait-Log 'load_end id=1' 10
            ctl 'new http://127.0.0.1:8765/page2.html' 0; $null = Wait-Log 'load_end id=2' 10
            $t0 = Get-Date
            ctl $how 0
            $exited = $script:Proc.WaitForExit(15000)
            $ms = [int]((Get-Date) - $t0).TotalMilliseconds
            Start-Sleep 0.5
            $left = @(Cef-Processes).Count
            $closes = @(Get-LogLines | Select-String 'on_before_close').Count
            $sd = (Get-LogLines | Select-String 'cef shutdown in ([\d.]+\w+)' | Select-Object -Last 1)
            $sd = if ($sd) { $sd.Matches[0].Groups[1].Value } else { 'none' }
            "{0,-3} {1,-8} run{2} exited={3} code={4} took={5}ms on_before_close={6} cef_shutdown={7} left={8}" -f $pump, $how, $r, $exited, $(if ($exited) { $script:Proc.ExitCode } else { '-' }), $ms, $closes, $sd, $left
            Stop-Spike -KeepServer
        }
    }
}
Stop-Spike
