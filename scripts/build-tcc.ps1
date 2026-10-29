# Builds the pinned tinycc into third_party/tcc for Windows x86_64 (MSVC).
# Layout matches scripts/build-tcc.sh so lust's build.rs and tcc::tcc_paths
# find the same files, with tcc.lib (the static library rustc links) and
# tcc.exe (the parallel compiler).
# Requires the "Desktop development with C++" Visual Studio workload.
$ErrorActionPreference = 'Stop'
$commit = '43c7708b85681a2fd4451c8a541af4494a8919b2'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$src = Join-Path $root 'third_party\tinycc'
$out = Join-Path $root 'third_party\tcc'

function Ensure-Cl {
    if (Get-Command cl.exe -ErrorAction SilentlyContinue) { return }
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path $vswhere)) {
        throw 'cl.exe is not on PATH and Visual Studio was not found. Install the Desktop development with C++ workload, then run this script from a Developer PowerShell or from a normal PowerShell.'
    }
    $vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if (-not $vs) { throw 'Visual Studio is installed but the C++ tools (cl.exe) are not.' }
    $dll = Join-Path $vs 'Common7\Tools\Microsoft.VisualStudio.DevShell.dll'
    Import-Module $dll
    Enter-VsDevShell -VsInstallPath $vs -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64' | Out-Null
    if (-not (Get-Command cl.exe -ErrorAction SilentlyContinue)) {
        throw 'Enter-VsDevShell did not put cl.exe on PATH.'
    }
}

Ensure-Cl

if (-not (Test-Path (Join-Path $src '.git'))) {
    git clone --quiet https://github.com/TinyCC/tinycc.git $src
}
git -C $src fetch --quiet --depth 1 origin $commit
git -C $src checkout --quiet --force $commit

$stage = Join-Path ([System.IO.Path]::GetTempPath()) ("lust-tcc-stage-" + $PID)
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Path (Join-Path $stage 'bin') | Out-Null

Push-Location (Join-Path $src 'win32')
cmd /c "build-tcc.bat -c cl -t x86_64 -i `"$stage`" -b `"$stage\bin`""
if ($LASTEXITCODE -ne 0) { throw "build-tcc.bat failed with exit $LASTEXITCODE" }
Pop-Location

# Static libtcc for the MSVC linker. /MD matches the CRT rustc links.
# The bat's config.h is already in the tinycc root.
Push-Location $src
if (Test-Path libtcc_static.obj) { Remove-Item libtcc_static.obj }
if (Test-Path tcc.lib) { Remove-Item tcc.lib }
& cl.exe /nologo /c /O2 /MD /W2 /D_CRT_SECURE_NO_WARNINGS /DTCC_TARGET_PE /DTCC_TARGET_X86_64 libtcc.c /Folibtcc_static.obj
if ($LASTEXITCODE -ne 0) { throw 'cl.exe failed to compile libtcc.c' }
& lib.exe /nologo /OUT:tcc.lib libtcc_static.obj
if ($LASTEXITCODE -ne 0) { throw 'lib.exe failed to archive tcc.lib' }
Pop-Location

$rt = Join-Path $out 'lib\tcc'
New-Item -ItemType Directory -Force -Path (Join-Path $out 'bin'), (Join-Path $out 'lib'), (Join-Path $rt 'include') | Out-Null
Copy-Item (Join-Path $stage 'bin\tcc.exe') (Join-Path $out 'bin\tcc.exe') -Force
Copy-Item (Join-Path $src 'tcc.lib') (Join-Path $out 'lib\tcc.lib') -Force

$tcc1 = Get-ChildItem -Path (Join-Path $stage 'lib') -Filter '*libtcc1.a' -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $tcc1) { throw "libtcc1.a was not produced in $stage\lib" }
Copy-Item $tcc1.FullName (Join-Path $rt 'libtcc1.a') -Force
Get-ChildItem (Join-Path $stage 'lib') -File | ForEach-Object {
    Copy-Item $_.FullName (Join-Path $rt $_.Name) -Force
}
if (Test-Path (Join-Path $stage 'include')) {
    Copy-Item (Join-Path $stage 'include\*') (Join-Path $rt 'include') -Recurse -Force
}

Remove-Item -Recurse -Force $stage
Write-Host "tcc $commit installed in $out"
