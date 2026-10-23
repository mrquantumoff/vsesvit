# Item 6: Chrome accelerators typed into the page, per interception mode
# (SPIKE_KEYS = none | cmd | key | both). Keys are posted to the page HWND through the
# message queue. One fresh spike per case; prints how the key was handled and which
# visible Chrome-native toplevels it left behind.
param([string[]]$Modes = @('none', 'cmd', 'key', 'both'))
. "$PSScriptRoot\lib.ps1"
Start-PageServer
$keys = [ordered]@{ 'Ctrl+T' = 't ctrl'; 'Ctrl+N' = 'n ctrl'; 'Ctrl+W' = 'w ctrl'; 'Ctrl+Shift+T' = 't ctrl shift'; 'F11' = 'f11'; 'Ctrl+P' = 'p ctrl'; 'Ctrl+F' = 'f ctrl'; 'F12' = 'f12' }
foreach ($mode in $Modes) {
    foreach ($k in $keys.Keys) {
        $label = "acc-$mode-" + ($k -replace '[^A-Za-z0-9]', '')
        $null = Start-Spike -Label $label -Vars @{ SPIKE_KEYS = $mode }
        if (-not (Wait-Log 'load_end id=1' 10)) { "no load"; continue }
        Start-Sleep 0.5
        Mark
        ctl "keypost $($keys[$k]) to=page" 2.5
        ctl tops 0.3
        ctl state 0.3
        $since = @(Since)
        $acts = @($since | Select-String 'cef-ev: action (\w+) from page id=\d+ via (\w+)' | ForEach-Object { "$($_.Matches[0].Groups[1].Value)<-$($_.Matches[0].Groups[2].Value)" }) -join ' '
        $cmds = @($since | Select-String 'on_chrome_command id=\d+ cmd=\d+ \((\w+)\)' | ForEach-Object { $_.Matches[0].Groups[1].Value }) -join ' '
        $native = @($since | Select-String '^0x\w+ "(Chrome_WidgetWin_\d|#32770)" "([^"]*)" (\S+) visible' | ForEach-Object { "$($_.Matches[0].Groups[1].Value):'$($_.Matches[0].Groups[2].Value)'" }) -join ' '
        $tabs = @($since | Select-String '^\s+tab \d+:').Count
        $page = @($since | Select-String 'page: keydown key=(\w+)' | ForEach-Object { $_.Matches[0].Groups[1].Value }) -join ','
        $other = @($since | Select-String 'fullscreen id|gpui: fullscreen|shell would handle|devtools' | ForEach-Object { ($_.Line -replace '^\[\s*[\d.]+\]\s*', '') }) -join ' | '
        "{0,-5} {1,-13} actions=[{2}] chrome_cmds=[{3}] page_saw=[{4}] tabs={5} native_visible=[{6}] {7}" -f $mode, $k, $acts, $cmds, $page, $tabs, $native, $other
        ctl quit 0
        $null = $script:Proc.WaitForExit(10000)
        Stop-Spike -KeepServer
    }
}
Stop-Spike
