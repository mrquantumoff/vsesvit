# The fail-closed guard: the spike refuses every desktop but "vsesvit-extwin" (never the input
# desktop). Negative cases run on other private desktops, so nothing touches the user's desktop.
. "$PSScriptRoot\lib.ps1"
$L = 'guard'
Remove-Item -Recurse -Force "$script:Scratch\runs\$L" -ErrorAction SilentlyContinue
foreach ($case in @(
        @{ name = 'other-private-desktop'; desk = 'vsesvit-other'; vars = @{} },
        @{ name = 'strict-on-right-desktop'; desk = $script:Desk; vars = @{ SPIKE_STRICT = '1' } },
        @{ name = 'right-desktop'; desk = $script:Desk; vars = @{} })) {
    $r = Start-Profile -P A -Label "$L-$($case.name)" -Desktop $case.desk -Vars $case.vars
    $g = (Log-Lines A "$L-$($case.name)" | Select-String 'GUARD|guard ok' | Select-Object -First 1).Line
    "$($case.name): $r :: $g"
    if ($r -like 'profile*') { Stop-Profile -P A -Label "$L-$($case.name)" }
}
Kill-All
