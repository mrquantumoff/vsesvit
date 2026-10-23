# Lay out the sandboxed variant: CEF's bootstrap.exe renamed to winspike.exe next
# to winspike.dll (exports RunWinMain) and the CEF runtime files. Idempotent.
. "$PSScriptRoot\lib.ps1"
$dst = "$script:Scratch\bundle"
New-Item -ItemType Directory -Force -Path $dst | Out-Null
Get-ChildItem $script:Target -File | Where-Object { $_.Extension -in '.dll', '.pak', '.dat', '.bin', '.json' } | Copy-Item -Destination $dst -Force
Copy-Item "$script:Target\locales" -Destination $dst -Recurse -Force
Copy-Item "$script:Target\bootstrap.exe" "$dst\winspike.exe" -Force
Get-ChildItem $dst | Measure-Object | ForEach-Object { "bundle: $($_.Count) entries in $dst" }
