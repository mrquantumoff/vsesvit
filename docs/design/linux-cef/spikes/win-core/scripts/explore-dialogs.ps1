# Item 8: popups, JS dialogs, file chooser, downloads and permission prompts, with CEF's
# default handling (SPIKE_DIALOGS=default) and with the shell's handlers (=shell).
# One fresh spike per case; prints the handler lines and any native toplevels that appeared.
param([string[]]$Modes = @('default', 'shell'), [string[]]$Cases = @('open', 'blank', 'alert', 'confirm', 'prompt', 'file', 'dl', 'geo', 'notif'), [hashtable]$Extra = @{}, [string]$Tag = '')
. "$PSScriptRoot\lib.ps1"
Start-PageServer
foreach ($mode in $Modes) {
    foreach ($case in $Cases) {
        $v = @{ SPIKE_DIALOGS = $mode }; foreach ($k in $Extra.Keys) { $v[$k] = $Extra[$k] }
        $null = Start-Spike -Label "dlg$Tag-$mode-$case" -Vars $v
        if (-not (Wait-Log 'load_end id=1' 10)) { "no load"; continue }
        ctl tops 0.3
        Mark
        ctl "clickel $case" 2.5
        ctl tops 0.3
        ctl "shot $script:Run\after.png" 0.5
        ctl state 0.3
        "=== mode=$mode case=$case"
        Since | Select-String -Pattern 'on_before_popup|tab \d+ <- browser|on_jsdialog|js dialog|on_file_dialog|file dialog|download|permission|page: |process toplevels|^0x' |
            Where-Object { $_.Line -notmatch 'keydown|visibility' } | ForEach-Object { "  $($_.Line)" }
        ctl quit 0
        $null = $script:Proc.WaitForExit(10000)
        Stop-Spike -KeepServer
    }
}
Stop-Spike
