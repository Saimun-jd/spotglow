Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class WinCheck {
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc lpEnumFunc, IntPtr lParam);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);
    [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr hWnd, StringBuilder lpString, int nMaxCount);
    [DllImport("user32.dll")] public static extern int GetClassName(IntPtr hWnd, StringBuilder lpClassName, int nMaxCount);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern long GetWindowLong(IntPtr hWnd, int nIndex);
    [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr hwnd, int dwAttribute, out RECT pvAttribute, int cbAttribute);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll", SetLastError = true)] public static extern IntPtr OpenDesktop(string lpszDesktop, uint dwFlags, bool fInherit, uint dwDesiredAccess);
    [DllImport("user32.dll", SetLastError = true)] public static extern bool SetThreadDesktop(IntPtr hDesktop);
}
"@

$hDefault = [WinCheck]::OpenDesktop("Default", 0, $false, 0x01FF)
if ($hDefault -ne [IntPtr]::Zero) {
    [WinCheck]::SetThreadDesktop($hDefault) | Out-Null
}

$pids = (Get-Process -Name "spotglow", "spotify" -ErrorAction SilentlyContinue)
Write-Host "Processes found:"
$pids | ForEach-Object { Write-Host "  PID $($_.Id): $($_.ProcessName)" }
$targetPids = $pids.Id

[WinCheck]::EnumWindows({
    param($hwnd, $lparam)
    [uint32]$p = 0
    [WinCheck]::GetWindowThreadProcessId($hwnd, [ref]$p) | Out-Null
    if ($targetPids -contains $p) {
        $sbTitle = New-Object System.Text.StringBuilder 512
        [WinCheck]::GetWindowText($hwnd, $sbTitle, 512) | Out-Null
        $sbClass = New-Object System.Text.StringBuilder 256
        [WinCheck]::GetClassName($hwnd, $sbClass, 256) | Out-Null
        $vis = [WinCheck]::IsWindowVisible($hwnd)
        $rect = New-Object WinCheck+RECT
        $hr = [WinCheck]::DwmGetWindowAttribute($hwnd, 9, [ref]$rect, 16)
        $exStyle = [WinCheck]::GetWindowLong($hwnd, -20)
        $style = [WinCheck]::GetWindowLong($hwnd, -16)
        $pName = (Get-Process -Id $p -ErrorAction SilentlyContinue).ProcessName
        Write-Host "HWND: $hwnd ($pName, PID $p) | Vis: $vis | Rect: [$($rect.Left),$($rect.Top),$($rect.Right),$($rect.Bottom)] ($($rect.Right - $rect.Left)x$($rect.Bottom - $rect.Top)) | Style: 0x$($style.ToString('X')) | ExStyle: 0x$($exStyle.ToString('X')) | Class: '$($sbClass.ToString())' | Title: '$($sbTitle.ToString())'"
    }
    return $true
}, [IntPtr]::Zero) | Out-Null
