# Lay out the sandboxed variant: CEF's bootstrap.exe renamed to extwin.exe next to extwin.dll
# (exports RunWinMain) and the CEF runtime files. Idempotent. deskrun.exe stays in the target dir.
. "$PSScriptRoot\lib.ps1"
$dst = $script:Bundle
New-Item -ItemType Directory -Force -Path $dst | Out-Null
Get-ChildItem $script:Target -File | Where-Object { $_.Extension -in '.dll', '.pak', '.dat', '.bin', '.json' } | Copy-Item -Destination $dst -Force
Copy-Item "$script:Target\locales" -Destination $dst -Recurse -Force
Copy-Item "$script:Target\bootstrap.exe" "$dst\extwin.exe" -Force
"bundle: $((Get-ChildItem $dst | Measure-Object).Count) entries in $dst"
