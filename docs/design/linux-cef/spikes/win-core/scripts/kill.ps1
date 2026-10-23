# Kill every spike process (browser, helpers, page server).
. "$PSScriptRoot\lib.ps1"
Stop-Spike
"spike processes left: $((Spike-Processes | Measure-Object).Count)"
