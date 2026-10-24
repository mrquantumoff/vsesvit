# Item 1 (Windows), registry-override route continued (run q1-routes.ps1 W4 first, label q1-W4):
#  1. acknowledge by enabling once from a hidden chrome://extensions page (no dialog?), restart: stays?
#  2. live install: add a registry subkey while the profile runs
#  3. pre-acknowledge: set extensions.settings.<id>.ack_external through RequestContext::SetPreference
#     before the subkey appears, then add it: installed enabled?
#  4. pre-acknowledge by editing Secure Preferences on disk before start (tracked prefs on Windows)
. "$PSScriptRoot\lib.ps1"
$L = 'q1-W4'; $P = 'A'
$ov = @{ SPIKE_REG_OVERRIDE = "Software\VsesvitSpikeExtWin\$L-$P" }
$listJs = 'chrome.developerPrivate.getExtensionsInfo({includeDisabled:true,includeTerminated:true}).then(a=>a.map(e=>[e.name,e.version,e.state,e.disableReasons&&Object.keys(e.disableReasons).filter(k=>e.disableReasons[k]).join("+")].join(" ")))'
function Mgmt { $null = c -P $P -Line "@20 hidden mgmt chrome chrome://extensions"; Start-Sleep -Milliseconds 500 }
function List { c -P $P -Line "@20 ev mgmt $listJs" }
function RegAdd($name, $ver) {
    $id = $script:Ids.$name; $crx = Archive $P $L $name $ver
    $k = "HKCU:\Software\VsesvitSpikeExtWin\$L-$P\Software\Google\Chrome\Extensions\$id"
    New-Item -Path $k -Force | Out-Null
    New-ItemProperty -Path $k -Name path -Value $crx -PropertyType String -Force | Out-Null
    New-ItemProperty -Path $k -Name version -Value $ver -PropertyType String -Force | Out-Null
}

Say "start (restart of q1-W4/A): $(Start-Profile -P $P -Label $L -Vars $ov)"
Mgmt
Say "list before: $(List)"
foreach ($n in 'H', 'C', 'D', 'S') {
    Say "enable $n from chrome://extensions: $(c -P $P -Line "@20 ev mgmt chrome.management.setEnabled('$($script:Ids.$n)', true).then(()=>'ok', e=>'ERR '+e.message)")"
}
Start-Sleep -Seconds 2
Say "list after enable: $(List)"
Say "toplevels: $(c -P $P -Line wtops)"
Say "bridge: $(Bridge $P)"
Say "helper list: $(Call $P '{op:"list"}')"
Say "pref read extensions.settings (len): $((c -P $P -Line 'pref extensions.settings').Length)"
Say "setpref dotted path extensions.settings.<E>.ack_external: $(c -P $P -Line "setpref extensions.settings.$($script:Ids.E).ack_external true")"
# 2 + 3: pre-acknowledge E through SetPreference, then add its registry subkey while running.
$set = (c -P $P -Line 'pref extensions.settings')
$json = $set.Substring(3) | ConvertFrom-Json -AsHashtable
$json[$script:Ids.E] = @{ ack_external = $true }
$compact = $json | ConvertTo-Json -Compress -Depth 50
Set-Content "$(RunDir $P $L)\settings-pre.json" $compact
Say "setpref extensions.settings (+E ack_external): $(c -P $P -Line "setpref extensions.settings $compact")"
RegAdd 'E' '1.0'
Start-Sleep -Seconds 6
Say "list after live E (pre-acked): $(List)"
# live add without pre-ack: B's... use C 2.0? No: a new id is needed; reuse D? Use version bump of C to see live update.
Say "stop: $(Stop-Profile -P $P -Label $L)"
Say "prefs after run:"; Prefs $P $L
Say "restart: $(Start-Profile -P $P -Label $L -Vars $ov)"
Mgmt
Say "list after restart: $(List)"
Say "reports: $((Reports $P $L) -join ' | ')"
Say "stop: $(Stop-Profile -P $P -Label $L)"
Kill-All
