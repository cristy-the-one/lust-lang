# Builds the Windows x86_64 release package:
#   dist/lust-<version>-x86_64-windows.zip
#   dist/lust-<version>-x86_64-windows.zip.sha256
#
#   powershell -File scripts/release.ps1            # build, then pack
#   powershell -File scripts/release.ps1 -NoBuild   # pack target\release
#
# Layout matches scripts/release.sh (lust finds lib\lust next to its executable):
#   bin\lust.exe, bin\lust-core.exe
#   lib\lust\tcc\bin\tcc.exe
#   lib\lust\tcc\lib\          libtcc1.a and tcc's headers
#   lib\lust\library\          std and adapter sources
#   LICENSE, tcc-COPYING
param([switch]$NoBuild)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $root
$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.*)"' | Select-Object -First 1).Matches.Groups[1].Value
if (-not $version) { throw 'version not found in Cargo.toml' }

$tcc = if ($env:LUST_TCC_HOME) { $env:LUST_TCC_HOME } else { Join-Path $root 'third_party\tcc' }
if (-not (Test-Path (Join-Path $tcc 'bin\tcc.exe')) -or -not (Test-Path (Join-Path $tcc 'lib\tcc\libtcc1.a'))) {
    & (Join-Path $PSScriptRoot 'build-tcc.ps1')
    $tcc = Join-Path $root 'third_party\tcc'
}
if (-not $NoBuild) {
    $env:LUST_TCC_HOME = $tcc
    cargo build --release --locked --bin lust --bin lust-core
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed with exit $LASTEXITCODE" }
}

$name = "lust-$version-x86_64-windows"
$dist = Join-Path $root 'dist'
$stage = Join-Path $dist $name
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force -Path (Join-Path $stage 'bin'), (Join-Path $stage 'lib\lust\tcc\bin'), (Join-Path $stage 'lib\lust\tcc\lib') | Out-Null
Copy-Item (Join-Path $root 'target\release\lust.exe'), (Join-Path $root 'target\release\lust-core.exe') (Join-Path $stage 'bin')
Copy-Item (Join-Path $tcc 'bin\tcc.exe') (Join-Path $stage 'lib\lust\tcc\bin')
Copy-Item (Join-Path $tcc 'lib\tcc\*') (Join-Path $stage 'lib\lust\tcc\lib') -Recurse -Force
Copy-Item (Join-Path $root 'library') (Join-Path $stage 'lib\lust\library') -Recurse -Force
Copy-Item (Join-Path $root 'README.md'), (Join-Path $root 'LICENSE') $stage
$copying = Join-Path $root 'third_party\tinycc\COPYING'
if (Test-Path $copying) { Copy-Item $copying (Join-Path $stage 'tcc-COPYING') }

$zip = Join-Path $dist "$name.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
# Windows 10 and later ship bsdtar, which writes a zip with -a.
& tar.exe -a -c -f $zip -C $dist $name
if ($LASTEXITCODE -ne 0) { throw "tar failed with exit $LASTEXITCODE" }
Remove-Item -Recurse -Force $stage
$hash = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLower()
$leaf = Split-Path $zip -Leaf
Set-Content -Path "$zip.sha256" -Value "$hash  $leaf" -Encoding ascii -NoNewline
Add-Content -Path "$zip.sha256" -Value "" -Encoding ascii
Write-Host "$hash  $leaf"
Write-Host "wrote $zip"
