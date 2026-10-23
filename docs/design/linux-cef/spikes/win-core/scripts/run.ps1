# Start the spike off-screen with the page server. Usage:
#   .\run.ps1 [-Label name] [-Urls "http://127.0.0.1:8765/page1.html https://example.org"] [-Vars @{SPIKE_PUMP='mt'}]
# Then drive it with ctl from lib.ps1:  . .\lib.ps1; ctl "state"; ctl "quit"
param([string]$Label = 'run', [string]$Urls = 'http://127.0.0.1:8765/page1.html', [hashtable]$Vars = @{})
. "$PSScriptRoot\lib.ps1"
Start-PageServer
$p = Start-Spike -Label $Label -Urls $Urls -Vars $Vars
"spike pid $($p.Id), log $script:Log"
