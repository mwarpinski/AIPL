# Packs the Windows release build into the zip install.ps1 downloads:
#
#   powershell -File tools\package_release.ps1 [OUT_DIR]      # default: target\dist
#
# It packs target\release\aipl.exe and aipl-run.exe (run `cargo build
# --release --locked` first) and the documents and examples, as
# OUT_DIR\aipl-TARGET.zip with OUT_DIR\aipl-TARGET.zip.sha256 beside it, where
# TARGET is x86_64-pc-windows-msvc. The zip holds one folder, aipl-TARGET: bin\
# (the two programs) and share\ (README, the specification, the prompt guide,
# the licence, and the examples), as tools/package_release.sh lays out the
# tarballs for Linux and macOS.
param([string]$Out = 'target\dist')
$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$target = 'x86_64-pc-windows-msvc'
foreach ($b in 'aipl.exe', 'aipl-run.exe') {
    if (-not (Test-Path (Join-Path $root "target\release\$b"))) { throw "package_release: build target\release\$b first" }
}
$name = "aipl-$target"
$out = if ([IO.Path]::IsPathRooted($Out)) { $Out } else { Join-Path $root $Out }
$stage = Join-Path $out "stage\$name"
if (Test-Path (Join-Path $out 'stage')) { Remove-Item -Recurse -Force (Join-Path $out 'stage') }
New-Item -ItemType Directory -Force (Join-Path $stage 'bin'), (Join-Path $stage 'share') | Out-Null
Copy-Item (Join-Path $root 'target\release\aipl.exe'), (Join-Path $root 'target\release\aipl-run.exe') (Join-Path $stage 'bin')
foreach ($f in 'README.md', 'AIPL_SPEC.md', 'PROMPT_GUIDE_FOR_AIS.md', 'LICENSE') { Copy-Item (Join-Path $root $f) (Join-Path $stage 'share') }
Copy-Item -Recurse (Join-Path $root 'examples') (Join-Path $stage 'share\examples')

$zip = Join-Path $out "$name.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path $stage -DestinationPath $zip
Remove-Item -Recurse -Force (Join-Path $out 'stage')
$hash = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLower()
# one line ending in LF, not the CRLF that Set-Content writes: `sha256sum -c` and the release workflow compare it exactly
[System.IO.File]::WriteAllText("$zip.sha256", "$hash  $name.zip`n")
"package_release: $zip"
