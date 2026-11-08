# Start `lust watch` in its own console, or send that console Ctrl-C.
# A Git bash background job shares the test's console, so Ctrl-C would also
# hit the test. taskkill without /F does not deliver Ctrl-C at all.
param(
    [Parameter(Mandatory = $true)][ValidateSet('start', 'ctrl')][string]$Action,
    [string]$Exe = '',
    [string]$WorkDir = '',
    [string]$ErrFile = '',
    [string]$OutFile = '',
    [uint32]$Target = 0
)

$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public class LustWin {
  [StructLayout(LayoutKind.Explicit, Size = 24)]
  public struct Sec {
    [FieldOffset(0)] public int nLength;
    [FieldOffset(8)] public IntPtr sd;
    [FieldOffset(16)] public int inherit;
  }
  // STARTUPINFOW on x64.
  [StructLayout(LayoutKind.Explicit, Size = 104)]
  public struct SI {
    [FieldOffset(0)] public int cb;
    [FieldOffset(8)] public IntPtr reserved;
    [FieldOffset(16)] public IntPtr desktop;
    [FieldOffset(24)] public IntPtr title;
    [FieldOffset(32)] public int x;
    [FieldOffset(36)] public int y;
    [FieldOffset(40)] public int xSize;
    [FieldOffset(44)] public int ySize;
    [FieldOffset(48)] public int xCount;
    [FieldOffset(52)] public int yCount;
    [FieldOffset(56)] public int fill;
    [FieldOffset(60)] public int flags;
    [FieldOffset(64)] public short show;
    [FieldOffset(66)] public short reserved2;
    [FieldOffset(72)] public IntPtr reserved3;
    [FieldOffset(80)] public IntPtr stdin;
    [FieldOffset(88)] public IntPtr stdout;
    [FieldOffset(96)] public IntPtr stderr;
  }
  [StructLayout(LayoutKind.Sequential)]
  public struct PI {
    public IntPtr process, thread;
    public int pid, tid;
  }
  [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
  public static extern IntPtr CreateFile(string name, uint access, uint share, ref Sec sa, uint disp, uint flags, IntPtr template);
  [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
  public static extern bool CreateProcess(string app, StringBuilder cmd, IntPtr pa, IntPtr ta, bool inherit, uint flags, IntPtr env, string dir, ref SI si, out PI pi);
  [DllImport("kernel32.dll", SetLastError = true)]
  public static extern bool CloseHandle(IntPtr h);
  [DllImport("kernel32.dll", SetLastError = true)]
  public static extern bool FreeConsole();
  [DllImport("kernel32.dll", SetLastError = true)]
  public static extern bool AttachConsole(uint id);
  [DllImport("kernel32.dll", SetLastError = true)]
  public static extern bool SetConsoleCtrlHandler(IntPtr h, bool add);
  [DllImport("kernel32.dll", SetLastError = true)]
  public static extern bool GenerateConsoleCtrlEvent(uint ev, uint group);
  [DllImport("kernel32.dll", SetLastError = true)]
  public static extern IntPtr GetStdHandle(int n);
  [DllImport("kernel32.dll", SetLastError = true)]
  public static extern bool SetHandleInformation(IntPtr h, uint mask, uint flags);
}
"@

if ($Action -eq 'ctrl') {
    [LustWin]::FreeConsole() | Out-Null
    if (-not [LustWin]::AttachConsole($Target)) {
        Write-Error "AttachConsole $Target failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
        exit 2
    }
    # This helper is attached to the watcher's console. Ignore the event here.
    [LustWin]::SetConsoleCtrlHandler([IntPtr]::Zero, $true) | Out-Null
    if (-not [LustWin]::GenerateConsoleCtrlEvent(0, 0)) {
        Write-Error "GenerateConsoleCtrlEvent failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
        exit 3
    }
    Start-Sleep -Milliseconds 300
    [LustWin]::FreeConsole() | Out-Null
    exit 0
}

$sa = New-Object LustWin+Sec
$sa.nLength = 24
$sa.inherit = 1
$invalid = [IntPtr]::new(-1)
function Open-Inherit([string]$path, [int64]$access, [uint32]$disp) {
    # 0x80000000 does not fit in a signed Int32, which is what a bare
    # PowerShell numeric argument becomes. Keep the low 32 bits.
    $uaccess = [uint32]($access -band 4294967295)
    $local = $script:sa
    $h = [LustWin]::CreateFile($path, $uaccess, 7, [ref]$local, $disp, 0, [IntPtr]::Zero)
    if ($h -eq $script:invalid -or $h -eq [IntPtr]::Zero) {
        throw "CreateFile $path failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
    }
    return $h
}
$stdin = Open-Inherit 'NUL' 0x80000000 3
$stdout = Open-Inherit $OutFile 0x40000000 2
$stderr = Open-Inherit $ErrFile 0x40000000 2
$si = New-Object LustWin+SI
$si.cb = 104
$si.flags = 0x101
$si.show = 0
$si.stdin = $stdin
$si.stdout = $stdout
$si.stderr = $stderr
# bInheritHandles is true so the three files above reach the watcher.
# This process's own stdin, stdout, and stderr must not go with them:
# the shell is reading our stdout for the pid, and a watcher that
# inherits that pipe holds it open for as long as it runs.
foreach ($n in -10, -11, -12) {
    $h = [LustWin]::GetStdHandle($n)
    if ($h -ne [IntPtr]::Zero -and $h -ne $invalid) {
        [void][LustWin]::SetHandleInformation($h, 1, 0)
    }
}
$pi = New-Object LustWin+PI
$cmd = New-Object System.Text.StringBuilder 1024
[void]$cmd.Append('"')
[void]$cmd.Append($Exe)
[void]$cmd.Append('" watch check hello.rs')
$ok = [LustWin]::CreateProcess($Exe, $cmd, [IntPtr]::Zero, [IntPtr]::Zero, $true, 0x10, [IntPtr]::Zero, $WorkDir, [ref]$si, [ref]$pi)
$winerr = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
[LustWin]::CloseHandle($stdin) | Out-Null
[LustWin]::CloseHandle($stdout) | Out-Null
[LustWin]::CloseHandle($stderr) | Out-Null
if (-not $ok) {
    Write-Error "CreateProcess failed: $winerr"
    exit 1
}
[LustWin]::CloseHandle($pi.process) | Out-Null
[LustWin]::CloseHandle($pi.thread) | Out-Null
Write-Output $pi.pid
