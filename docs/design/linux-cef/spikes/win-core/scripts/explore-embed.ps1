# Item 3: the CEF child follows window resize and GPUI layout changes; host HWND vs
# direct parenting; Chromium's scale against GPUI's.
. "$PSScriptRoot\lib.ps1"
Start-PageServer
function M([object[]]$Lines, [string]$Pattern, [int]$G = 1) {
    $m = @($Lines | Select-String $Pattern)
    if ($m.Count) { $m[-1].Matches[0].Groups[$G].Value } else { '?' }
}
function Geo {
    ctl state 0.3; ctl dpi 0.5; ctl content 0.5
    $all = @(Since)
    $applied = M $all 'applied_host=Some\(PRect \{([^}]*)\}\)'
    $child = M $all 'applied=Some\(\(\d+, PRect \{([^}]*)\}\)'
    $dpr = M $all 'page: dpr=(\S+) inner=(\S+)' 1
    $inner = M $all 'page: dpr=(\S+) inner=(\S+)' 2
    $cont = M $all 'non_background=([\d.]+)'
    "  host=[$applied] child=[$child] dpr=$dpr inner=$inner page_visible=$cont"
}
foreach ($v in @(@{ L = 'host'; V = @{} }, @{ L = 'direct'; V = @{ SPIKE_HOST = 'direct' } }, @{ L = 'resize-inline'; V = @{ SPIKE_RESIZE_INLINE = '1' } })) {
    $null = Start-Spike -Label "embed-$($v.L)" -Vars $v.V
    $null = Wait-Log 'load_end id=1' 10; Start-Sleep 0.5
    "=== $($v.L)"
    Mark; "--- initial 1280x800"; Geo
    Mark; ctl 'menu region' 0.8; ctl 'hittest 1133 180' 0.2; ctl 'probe 1133 180' 0.3; ctl 'menu off' 0.5; ctl 'hittest 1133 180' 0.2
    "--- region cut-out at the menu centre, then removed"
    Since | Select-String 'win: region|hittest \(|probe \(' | ForEach-Object { '  ' + $_.Line }
    Mark; ctl 'resize 1000 650' 1; "--- resize 1000x650"; Geo
    ctl "shot $script:Run\resized.png" 0.3
    Mark; ctl bar 1; "--- extra 40 px bar above the content"; Geo
    ctl "shot $script:Run\bar.png" 0.3
    ctl quit 0; $null = $script:Proc.WaitForExit(15000)
    Stop-Spike -KeepServer
}
foreach ($scale in '1', '2') {
    $null = Start-Spike -Label "embed-cefscale-$scale" -Vars @{ SPIKE_CEF_SCALE = $scale }
    $null = Wait-Log 'load_end id=1' 10; Start-Sleep 0.5
    Mark; "=== --force-device-scale-factor=$scale (GPUI stays at the monitor's 1.75)"; Geo
    ctl "shot $script:Run\shot.png" 0.3
    ctl quit 0; $null = $script:Proc.WaitForExit(15000)
    Stop-Spike -KeepServer
}
Stop-Spike
