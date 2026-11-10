# Smoke test of the Windows release package (scripts/release.ps1): unpacks it away from the
# checkout and checks, runs and builds one program with the packaged lust alone. A package
# whose tcc cannot find its headers, or whose std sources are missing, fails here and is not
# published (.github/workflows/release.yml).
#   pwsh -File scripts/release-smoke.ps1 dist\lust-<version>-x86_64-windows.zip
param([Parameter(Mandatory = $true)][string]$Package)
$ErrorActionPreference = 'Stop'
$pkg = (Resolve-Path $Package).Path
$work = Join-Path ([System.IO.Path]::GetTempPath()) ("lust-smoke-" + [System.Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $work | Out-Null
try {
    Expand-Archive -Path $pkg -DestinationPath $work -Force
    $root = Join-Path $work ([System.IO.Path]::GetFileNameWithoutExtension($pkg))
    $lust = Join-Path $root 'bin\lust.exe'
    if (-not (Test-Path $lust)) { throw "release-smoke: no bin\lust.exe in $pkg" }
    # Nothing of the checkout or of a dev build may help the packaged binaries.
    Remove-Item Env:LUST_TCC_HOME, Env:LUST_STD_SRC -ErrorAction SilentlyContinue
    $env:LUST_NO_DAEMON = '1'
    $env:LUST_CACHE_DIR = Join-Path $work 'cache'
    $try = Join-Path $work 'try'
    New-Item -ItemType Directory -Force -Path $try | Out-Null
    Set-Location $try
    Set-Content -Path hello.rs -Encoding utf8 -Value @'
use std::collections::BTreeMap;
fn main() {
    let v: Vec<u32> = (1..=4).collect();
    let mut m = BTreeMap::new();
    m.insert("sum", v.iter().sum::<u32>());
    println!("hello {:?}", m);
}
'@
    $want = 'hello {"sum": 10}'
    & $lust --version
    if ($LASTEXITCODE -ne 0) { throw "release-smoke: lust --version failed with exit $LASTEXITCODE" }
    & $lust check hello.rs
    if ($LASTEXITCODE -ne 0) { throw "release-smoke: lust check failed with exit $LASTEXITCODE" }
    $got = & $lust run hello.rs
    if ($LASTEXITCODE -ne 0 -or $got -ne $want) { throw "release-smoke: lust run printed: $got (exit $LASTEXITCODE)" }
    & $lust build hello.rs -o hello.exe
    if ($LASTEXITCODE -ne 0) { throw "release-smoke: lust build failed with exit $LASTEXITCODE" }
    $got = & .\hello.exe
    if ($got -ne $want) { throw "release-smoke: the built program printed: $got" }
    Write-Host "release-smoke: $(Split-Path $pkg -Leaf) ok"
}
finally {
    Set-Location $PSScriptRoot
    Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}
