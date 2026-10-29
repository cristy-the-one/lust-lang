# Builds the pinned tinycc into third_party/tcc for Windows x86_64 (MSVC).
# Layout matches scripts/build-tcc.sh so build.rs and tcc::tcc_paths find the
# same files, plus tcc.lib (the static library rustc links) and tcc.exe.
#
# win32/build-tcc.bat writes
#   #define CONFIG_TCCDIR "%TCCDIR:\=/%"
# and cmd drops the quotes, so MSVC sees tcc_set_lib_path(s, C:/...) and
# stops with "missing ')' before identifier 'C'". This script writes config.h
# itself and compiles with cl.
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
    Import-Module (Join-Path $vs 'Common7\Tools\Microsoft.VisualStudio.DevShell.dll')
    Enter-VsDevShell -VsInstallPath $vs -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64' | Out-Null
    if (-not (Get-Command cl.exe -ErrorAction SilentlyContinue)) {
        throw 'Enter-VsDevShell did not put cl.exe on PATH.'
    }
}

function Invoke-Native {
    param([Parameter(Mandatory)][string]$File, [Parameter(ValueFromRemainingArguments)][string[]]$NativeArgs)
    & $File @NativeArgs
    if ($LASTEXITCODE -ne 0) { throw "$File $($NativeArgs -join ' ') failed with exit $LASTEXITCODE" }
}

Ensure-Cl

if (-not (Test-Path (Join-Path $src '.git'))) {
    git clone --quiet https://github.com/TinyCC/tinycc.git $src
}
git -C $src fetch --quiet --depth 1 origin $commit
git -C $src checkout --quiet --force $commit

$rtFwd = ((Join-Path $out 'lib\tcc') -replace '\\', '/')
@(
    '#define TCC_VERSION "0.9.28rc"'
    "#define CONFIG_TCCDIR `"$rtFwd`""
) | Set-Content -Encoding ascii -Path (Join-Path $src 'config.h')
Write-Host "config.h CONFIG_TCCDIR=$rtFwd"

$cflags = @(
    '/nologo', '/O2', '/MD', '/W2',
    '/D_CRT_SECURE_NO_WARNINGS',
    '/DTCC_TARGET_PE',
    '/DTCC_TARGET_X86_64'
)

Push-Location $src
Invoke-Native cl.exe @($cflags + @('/c', 'libtcc.c', '/Folibtcc.obj'))
Invoke-Native lib.exe @('/nologo', '/OUT:tcc.lib', 'libtcc.obj')
# tcc.c defaults to ONE_SOURCE, which includes libtcc.c. A second copy of
# those symbols must not be linked in.
Invoke-Native cl.exe @($cflags + @('tcc.c', '/Fetcc.exe'))
Pop-Location

$win32 = Join-Path $src 'win32'
$tcc = Join-Path $src 'tcc.exe'
# tcc -B. looks in win32/include. The shared headers (tccdefs.h and the rest)
# have to be there before libtcc1 is compiled. build-tcc.bat copies them at
# the same point.
$includeStage = Join-Path $win32 'include'
Copy-Item (Join-Path $src 'include\*.h') $includeStage -Force
if (Test-Path (Join-Path $src 'tcclib.h')) {
    Copy-Item (Join-Path $src 'tcclib.h') $includeStage -Force
}
Push-Location $win32
function Invoke-Tcc([string[]]$TccArgs) {
    & $tcc -B. @TccArgs
    if ($LASTEXITCODE -ne 0) { throw "tcc $($TccArgs -join ' ') failed with exit $LASTEXITCODE" }
}
foreach ($unit in @(
    '../lib/libtcc1.c', 'lib/crt1.c', 'lib/crt1w.c', 'lib/wincrt1.c', 'lib/wincrt1w.c',
    'lib/dllcrt1.c', 'lib/dllmain.c', 'lib/winex.c', 'lib/chkstk.S', '../lib/alloca.S',
    '../lib/alloca-bt.S', '../lib/stdatomic.c', '../lib/atomic.S', '../lib/builtin.c'
)) {
    Invoke-Tcc @('-c', $unit)
}
Invoke-Tcc (@('-ar', 'lib/libtcc1.a') + @(
    'libtcc1.o', 'crt1.o', 'crt1w.o', 'wincrt1.o', 'wincrt1w.o', 'dllcrt1.o',
    'dllmain.o', 'winex.o', 'chkstk.o', 'alloca.o', 'alloca-bt.o', 'stdatomic.o',
    'atomic.o', 'builtin.o'
))
Invoke-Tcc @('-c', '../lib/bcheck.c', '-o', 'lib/bcheck.o', '-bt', '-I..')
Invoke-Tcc @('-c', '../lib/bcheck.c', '-o', 'lib/bcheck_run.o', '-bt', '-I..', '-DBCHECK_RUN')
Invoke-Tcc @('-c', '../lib/bt-exe.c', '-o', 'lib/bt-exe.o')
Invoke-Tcc @('-c', '../lib/bt-log.c', '-o', 'lib/bt-log.o')
Invoke-Tcc @('-c', '../lib/bt-dll.c', '-o', 'lib/bt-dll.o')
Invoke-Tcc @('-c', '../lib/runmain.c', '-o', 'lib/runmain.o')
Pop-Location

$rt = Join-Path $out 'lib\tcc'
New-Item -ItemType Directory -Force -Path (Join-Path $out 'bin'), (Join-Path $out 'lib'), (Join-Path $rt 'include') | Out-Null
Copy-Item $tcc (Join-Path $out 'bin\tcc.exe') -Force
Copy-Item (Join-Path $src 'tcc.lib') (Join-Path $out 'lib\tcc.lib') -Force
Copy-Item (Join-Path $win32 'lib\libtcc1.a') (Join-Path $rt 'libtcc1.a') -Force
Get-ChildItem (Join-Path $win32 'lib') -File | ForEach-Object {
    Copy-Item $_.FullName (Join-Path $rt $_.Name) -Force
}
Copy-Item (Join-Path $includeStage '*') (Join-Path $rt 'include') -Recurse -Force

Write-Host "tcc $commit installed in $out"
