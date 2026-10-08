# GUI test on a real Windows desktop (CI): every window centred, Hide details shrinks the
# progress window. Prints each TurboDM window's position and saves screenshots.
param([string]$Exe)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System; using System.Collections.Generic; using System.Runtime.InteropServices; using System.Text;
public static class Win {
    public struct Rect { public int Left, Top, Right, Bottom; }
    delegate bool Each(IntPtr hwnd, IntPtr param);
    [DllImport("user32")] static extern bool EnumWindows(Each each, IntPtr param);
    [DllImport("user32")] static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32")] static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int size);
    [DllImport("user32")] static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("dwmapi")] static extern int DwmGetWindowAttribute(IntPtr hwnd, int attr, out Rect rect, int size);
    public static List<string> Of(uint[] pids) {
        var list = new List<string>();
        EnumWindows((h, p) => {
            uint pid; GetWindowThreadProcessId(h, out pid);
            if (Array.IndexOf(pids, pid) < 0 || !IsWindowVisible(h)) return true;
            var title = new StringBuilder(256); GetWindowText(h, title, 256);
            Rect r; DwmGetWindowAttribute(h, 9, out r, 16); // the frame as seen on screen
            list.Add(String.Format("{0}: {1},{2} {3}x{4} centre {5},{6}", title, r.Left, r.Top,
                r.Right - r.Left, r.Bottom - r.Top, (r.Left + r.Right) / 2, (r.Top + r.Bottom) / 2));
            return true;
        }, IntPtr.Zero);
        return list;
    }
}
"@
$screen = [System.Windows.Forms.Screen]::PrimaryScreen
function Report($name) {
    Start-Sleep 2
    $area = $screen.WorkingArea
    Write-Host "== $name (work area centre $(($area.Left + $area.Right) / 2),$(($area.Top + $area.Bottom) / 2))"
    [Win]::Of([uint32[]](Get-Process turbodm).Id) | ForEach-Object { Write-Host "   $_" }
    $b = $screen.Bounds
    $image = New-Object System.Drawing.Bitmap $b.Width, $b.Height
    [System.Drawing.Graphics]::FromImage($image).CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size)
    $image.Save("$PWD\gui-$name.png")
}
Remove-Item "$env:APPDATA\turbodm" -Recurse -ErrorAction SilentlyContinue # a first start
$shell = New-Object -ComObject WScript.Shell
$server = Start-Process python -ArgumentList "tools/slow_server.py", "8765", "209715200", "262144" -PassThru
Start-Process $Exe
Start-Sleep 5
Report "0-browsers" # first start: the "add to your browser" guide
if ($shell.AppActivate("Add TurboDM to your browser")) { Start-Sleep 1; $shell.SendKeys("{ENTER}") }
Report "1-main"
$shell.SendKeys("^,")
Report "2-preferences"
$shell.SendKeys("{ESC}")
$null = $shell.AppActivate("Preferences"); Start-Sleep 1; $shell.SendKeys("%{F4}")
Start-Process $Exe -ArgumentList "http://127.0.0.1:8765/file"
Start-Sleep 4
Report "3-add-dialog"
$null = $shell.AppActivate("Download File Info"); Start-Sleep 1; $shell.SendKeys("{ENTER}")
Start-Sleep 3
Report "4-progress"
$shell.SendKeys("%h")
Report "5-details-hidden"
$shell.SendKeys("%s")
Report "6-details-shown"
Stop-Process -Name turbodm
$server | Stop-Process
