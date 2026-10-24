# Item 1 (Windows): which install routes Chromium reads, per profile, with two profiles open at
# once whose sets conflict (A: H, C 1.0, D 1.0, S. B: H, C 2.0, E, S).
#   W1  <root_cache_path>\External Extensions\<id>.json   (Linux's route)
#   W2  <libcef dir>\extensions\external_extensions.json  (A only; global by construction)
#   W3  no route: what the user's own HKCU\Software\Google\Chrome\Extensions brings in (read only)
#   W4  HKCU redirected per process (RegOverridePredefKey) to HKCU\Software\VsesvitSpikeExtWin\<label>-<P>,
#       holding Software\Google\Chrome\Extensions\<id> {path, version}: the per-profile registry route
# Listing goes through a hidden Chrome-style chrome://extensions page (developerPrivate), so it
# works even when the helper did not install.
param([string[]]$Routes = @('W1', 'W2', 'W3', 'W4'))
. "$PSScriptRoot\lib.ps1"
$sets = @{ A = @(@('H', '1.0'), @('C', '1.0'), @('D', '1.0'), @('S', '1.0')); B = @(@('H', '1.0'), @('C', '2.0'), @('E', '1.0'), @('S', '1.0')) }
$listJs = 'chrome.developerPrivate.getExtensionsInfo({includeDisabled:true,includeTerminated:true}).then(a=>a.map(e=>[e.name,e.version,e.state,e.location,e.disableReasons&&Object.keys(e.disableReasons).filter(k=>e.disableReasons[k]).join("+")].join(" ")))'

function List($p) {
    $null = c -P $p -Line "@20 hidden mgmt chrome chrome://extensions"
    Start-Sleep -Milliseconds 800
    $r = c -P $p -Line "@20 ev mgmt $listJs"
    $null = c -P $p -Line 'closeh mgmt'
    $r
}
function Tops($p) { c -P $p -Line 'wtops' }

foreach ($route in $Routes) {
    $L = "q1-$route"
    Remove-Item -Recurse -Force "$script:Scratch\runs\$L" -ErrorAction SilentlyContinue
    $profiles = if ($route -eq 'W2' -or $route -eq 'W3') { @('A') } else { @('A', 'B') }
    $vars = @{}
    foreach ($p in $profiles) {
        $root = RootDir $p $L
        New-Item -ItemType Directory -Force -Path $root | Out-Null
        foreach ($e in $sets[$p]) {
            $crx = Archive $p $L $e[0] $e[1]
            $id = $script:Ids.($e[0])
            switch ($route) {
                'W1' {
                    New-Item -ItemType Directory -Force -Path "$root\External Extensions" | Out-Null
                    @{ external_crx = $crx; external_version = $e[1] } | ConvertTo-Json -Compress | Set-Content -Encoding ascii "$root\External Extensions\$id.json"
                }
                'W2' {
                    New-Item -ItemType Directory -Force -Path "$script:Bundle\extensions" | Out-Null
                    $j = if (Test-Path "$script:Bundle\extensions\external_extensions.json") { Get-Content "$script:Bundle\extensions\external_extensions.json" -Raw | ConvertFrom-Json -AsHashtable } else { @{} }
                    $j[$id] = @{ external_crx = $crx; external_version = $e[1] }
                    $j | ConvertTo-Json -Compress | Set-Content -Encoding ascii "$script:Bundle\extensions\external_extensions.json"
                }
                'W4' {
                    $k = "HKCU:\Software\VsesvitSpikeExtWin\$L-$p\Software\Google\Chrome\Extensions\$id"
                    New-Item -Path $k -Force | Out-Null
                    New-ItemProperty -Path $k -Name path -Value $crx -PropertyType String -Force | Out-Null
                    New-ItemProperty -Path $k -Name version -Value $e[1] -PropertyType String -Force | Out-Null
                }
            }
        }
    }
    Say "route $route"
    foreach ($p in $profiles) {
        $v = @{}
        if ($route -eq 'W4') { $v['SPIKE_REG_OVERRIDE'] = "Software\VsesvitSpikeExtWin\$L-$p" }
        Start-Profile -P $p -Label $L -Vars $v
    }
    Start-Sleep -Seconds 8
    foreach ($p in $profiles) {
        Say "$route $p list: $(List $p)"
        Say "$route $p toplevels: $(Tops $p)"
        Say "$route $p reports: $((Reports $p $L) -join ' | ')"
    }
    foreach ($p in $profiles) { Stop-Profile -P $p -Label $L }
    foreach ($p in $profiles) { Say "$route $p prefs:"; Prefs $p $L }
    foreach ($p in $profiles) { Say "$route $p winevents:"; Log-Lines $p $L | Select-String 'winevent: (show|create) .*top=true' | Select-Object -First 12 | ForEach-Object { $_.Line } }
    if ($route -eq 'W2') { Remove-Item -Force "$script:Bundle\extensions\external_extensions.json" }
}
Kill-All
