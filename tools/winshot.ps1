param($exe, $file, $out, $keys)
Add-Type -AssemblyName System.Drawing, System.Windows.Forms
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class Cap {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  public struct RECT { public int L, T, R, B; }
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindow(string cls, string title);
  public static int[] Rect(IntPtr h) { RECT r; GetWindowRect(h, out r); return new int[] { r.L, r.T, r.R, r.B }; }
}
"@
[Cap]::SetProcessDPIAware() | Out-Null
$p = Start-Process $exe -ArgumentList "`"$file`"" -PassThru
$n = 0; while ($p.MainWindowHandle -eq 0 -and $n -lt 300) { Start-Sleep -Milliseconds 30; $p.Refresh(); $n++ }
Start-Sleep 3; $hwnd = [Cap]::FindWindow([NullString]::Value, "Inkdown (native)"); if ($hwnd -eq [IntPtr]::Zero) { $hwnd = $p.MainWindowHandle }
$a = [Cap]::Rect($hwnd); $r = @{ L = $a[0]; T = $a[1]; R = $a[2]; B = $a[3] }
$b = New-Object System.Drawing.Bitmap ($r.R - $r.L), ($r.B - $r.T)
[Cap]::SetForegroundWindow($hwnd) | Out-Null; Start-Sleep -Milliseconds 700; $g = [System.Drawing.Graphics]::FromImage($b); $g.CopyFromScreen($r.L, $r.T, 0, 0, $b.Size); $g.Dispose(); "rect $($r.L),$($r.T) $($b.Width)x$($b.Height)"
$b.Save($out); $b.Dispose()
$p.CloseMainWindow() | Out-Null; Start-Sleep 1; if (-not $p.HasExited) { $p.Kill() }
"saved $out"




