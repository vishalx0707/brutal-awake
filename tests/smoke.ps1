param([string]$Exe = (Join-Path $PSScriptRoot '..\dist\BrutalAwake.exe'))
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class NativeSmoke {
    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr hwnd, int id);
    [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr hwnd, uint msg, IntPtr wp, IntPtr lp);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hwnd, uint msg, IntPtr wp, IntPtr lp);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int count);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out Rect rect);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hwnd, IntPtr dc, uint flags);
    [DllImport("user32.dll")] public static extern uint GetGuiResources(IntPtr process, uint flags);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hwnd, int cmd);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr hwnd, IntPtr after, int x, int y, int w, int h, uint flags);
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
}
'@
function Assert($condition, [string]$message) { if (!$condition) { throw $message } }
function Label($button) {
    $text = New-Object System.Text.StringBuilder 100
    [void][NativeSmoke]::GetWindowText($button, $text, 100)
    $text.ToString()
}
function Capture($hwnd, [string]$name) {
    $previousDpi = [NativeSmoke]::SetThreadDpiAwarenessContext([IntPtr]::new(-4))
    $rect = New-Object NativeSmoke+Rect
    [void][NativeSmoke]::GetWindowRect($hwnd, [ref]$rect)
    $bitmap = New-Object System.Drawing.Bitmap ($rect.Right-$rect.Left), ($rect.Bottom-$rect.Top)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $dc = $graphics.GetHdc()
    try { Assert ([NativeSmoke]::PrintWindow($hwnd, $dc, 2)) 'PrintWindow failed.' }
    finally { $graphics.ReleaseHdc($dc); $graphics.Dispose() }
    try { $bitmap.Save((Join-Path $PSScriptRoot "..\docs\$name.png")) } finally {
        $bitmap.Dispose()
        [void][NativeSmoke]::SetThreadDpiAwarenessContext($previousDpi)
    }
}

$before = (powercfg /query | Out-String)
Assert ($LASTEXITCODE -eq 0) 'Could not snapshot the active power plan.'
$app = Start-Process -FilePath (Resolve-Path -LiteralPath $Exe).Path -PassThru -WindowStyle Hidden
try {
    for ($attempt=0; $attempt -lt 50; $attempt++) {
        Start-Sleep -Milliseconds 100
        $app.Refresh()
        if ($app.MainWindowHandle -ne 0 -or $app.HasExited) { break }
    }
    # Hidden startup windows are excluded by Process.MainWindowHandle. Discover
    # this app's class directly in that case; never target another app by title.
    if ($app.MainWindowHandle -eq 0) {
        Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class FindSmoke {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string cls, string title);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
}
'@
        $hwnd = [FindSmoke]::FindWindow('BrutalAwakeNative', 'Brutal Awake')
        [uint32]$owner = 0
        [void][FindSmoke]::GetWindowThreadProcessId($hwnd, [ref]$owner)
        Assert ($owner -eq $app.Id) 'Another copy is running; close it before testing.'
    } else { $hwnd = $app.MainWindowHandle }
    Assert ($hwnd -ne 0) 'No native window created.'
    $button = [NativeSmoke]::GetDlgItem($hwnd, 100)
    Assert ((Label $button) -eq 'Keep awake: on') 'Startup did not turn ON.'
    Capture $hwnd 'native-on'
    $duplicate = Start-Process -FilePath (Resolve-Path -LiteralPath $Exe).Path -PassThru -WindowStyle Hidden
    Assert ($duplicate.WaitForExit(5000)) 'Duplicate instance did not exit.'
    Assert ((Label $button) -eq 'Keep awake: on') 'Duplicate changed the original toggle.'
    [void][NativeSmoke]::SendMessage($button, 0x00f5, [IntPtr]::Zero, [IntPtr]::Zero)
    Assert ((Label $button) -eq 'Keep awake: off') 'Toggle OFF failed.'
    Capture $hwnd 'native-off'
    [void][NativeSmoke]::SendMessage($button, 0x00f5, [IntPtr]::Zero, [IntPtr]::Zero)
    Assert ((Label $button) -eq 'Keep awake: on') 'Toggle ON failed.'
    [void][NativeSmoke]::PostMessage($button, 0x0100, [IntPtr]::new(32), [IntPtr]::Zero)
    [void][NativeSmoke]::PostMessage($button, 0x0101, [IntPtr]::new(32), [IntPtr]::Zero)
    Start-Sleep -Milliseconds 100
    Assert ((Label $button) -eq 'Keep awake: off') 'Space key did not switch OFF.'
    [void][NativeSmoke]::PostMessage($button, 0x0100, [IntPtr]::new(32), [IntPtr]::Zero)
    [void][NativeSmoke]::PostMessage($button, 0x0101, [IntPtr]::new(32), [IntPtr]::Zero)
    Start-Sleep -Milliseconds 100
    Assert ((Label $button) -eq 'Keep awake: on') 'Space key did not switch ON.'
    $gdiBefore = [NativeSmoke]::GetGuiResources($app.Handle, 0)
    $app.Refresh()
    $handlesBefore = $app.HandleCount
    for ($i=0; $i -lt 500; $i++) {
        [void][NativeSmoke]::SendMessage($button, 0x00f5, [IntPtr]::Zero, [IntPtr]::Zero)
        [void][NativeSmoke]::SendMessage($hwnd, 0x000f, [IntPtr]::Zero, [IntPtr]::Zero)
        [void][NativeSmoke]::SendMessage($button, 0x000f, [IntPtr]::Zero, [IntPtr]::Zero)
    }
    $gdiAfter = [NativeSmoke]::GetGuiResources($app.Handle, 0)
    Assert ($gdiAfter -le $gdiBefore+2) 'GDI resources grew across repeated toggles.'
    $app.Refresh()
    Assert ($app.HandleCount -le $handlesBefore+3) 'Handles grew across repeated toggles.'
    [void][NativeSmoke]::SetWindowPos($hwnd, [IntPtr]::Zero, 0, 0, 320, 440, 6)
    Capture $hwnd 'native-small'
    [void][NativeSmoke]::ShowWindow($hwnd, 6)
    Assert ((Label $button) -eq 'Keep awake: on') 'Minimizing changed the toggle.'
    $app.Refresh()
    $cpuBefore = $app.TotalProcessorTime.TotalMilliseconds
    $memoryBefore = $app.PrivateMemorySize64
    Start-Sleep -Seconds 30
    $app.Refresh()
    $cpuMs = $app.TotalProcessorTime.TotalMilliseconds-$cpuBefore
    $children = @(Get-CimInstance Win32_Process | Where-Object ParentProcessId -eq $app.Id)
    Assert ($children.Count -eq 0) 'Unexpected child processes.'
    $after = (powercfg /query | Out-String)
    Assert ($before -ceq $after) 'The active power plan changed.'
    $report = [ordered]@{
        exeBytes = (Get-Item -LiteralPath $Exe).Length
        workingSetMiB = [math]::Round($app.WorkingSet64/1MB, 2)
        privateMiBBeforeIdle = [math]::Round($memoryBefore/1MB, 2)
        privateMiBAfterIdle = [math]::Round($app.PrivateMemorySize64/1MB, 2)
        idleSeconds = 30
        idleCpuMilliseconds = $cpuMs
        threads = $app.Threads.Count
        childProcesses = $children.Count
        gdiBefore = $gdiBefore
        gdiAfter500Toggles = $gdiAfter
        powerPlanUnchanged = $true
        toggleAndSingleInstancePassed = $true
        keyboardTogglePassed = $true
    }
    [void][NativeSmoke]::PostMessage($hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)
    Assert ($app.WaitForExit(5000)) 'Closing left the app running.'
    $report['cleanExitPassed'] = $true
    Assert ($before -ceq (powercfg /query | Out-String)) 'Power plan changed on exit.'
    $report | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $PSScriptRoot '..\docs\verification.json')
    $report | ConvertTo-Json
} finally {
    if (!$app.HasExited) {
        if ($hwnd -ne 0) { [void][NativeSmoke]::PostMessage($hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) }
        if (!$app.WaitForExit(3000)) { $app.Kill() }
    }
}
