# Builds the Microsoft Store package (MSIX) for Inkdown.
#
#   pwsh packaging\windows\make-msix.ps1                      # dev identity, for a local look
#   pwsh packaging\windows\make-msix.ps1 -IdentityName "12345Publisher.Inkdown" `
#        -Publisher "CN=XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX" -PublisherDisplayName "Your Name"
#
# The three identity values come from Partner Center (Product identity page) after you
# reserve the app name. Store uploads do not need to be signed: the Store signs them.
param(
    [string]$IdentityName = 'Inkdown.MarkdownEditor',
    [string]$Publisher = 'CN=InkdownDev',
    [string]$PublisherDisplayName = 'Inkdown',
    [string]$Exe = '',
    [string]$Version = ''
)
$ErrorActionPreference = 'Stop'
$root = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
if (-not $Exe) { $Exe = Join-Path $root 'native\target\release\inkdown.exe' }
if (-not $Version) { $Version = ((Select-String -Path (Join-Path $root 'native\Cargo.toml') -Pattern '^version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value) }
$version4 = "$Version.0"
$kits = (Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows Kits\Installed Roots' -ErrorAction SilentlyContinue).KitsRoot10
if (-not $kits) { $kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10' }
$makeappx = Get-ChildItem (Join-Path $kits 'bin') -Recurse -Filter makeappx.exe | Where-Object { $_.FullName -match '\\x64\\' } | Sort-Object FullName | Select-Object -Last 1
if (-not $makeappx) { throw 'makeappx.exe not found. Install the Windows SDK.' }

$layout = Join-Path $root 'target\msix'
Remove-Item $layout -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$layout\Assets" | Out-Null
Copy-Item $Exe "$layout\inkdown.exe"

# ---- logos, drawn from the same artwork as the app icon
Add-Type -AssemblyName System.Drawing
function Save-Logo([int]$w, [int]$h, [string]$name, [double]$fill = 0.78) {
    $bmp = New-Object System.Drawing.Bitmap $w, $h, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = 'AntiAlias'; $g.PixelOffsetMode = 'HighQuality'; $g.Clear([System.Drawing.Color]::Transparent)
    $s = [float]([Math]::Min($w, $h) * $fill); $ox = [float](($w - $s) / 2); $oy = [float](($h - $s) / 2)
    $r = [float]($s * 0.23); $d = 2 * $r
    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $path.AddArc($ox, $oy, $d, $d, 180, 90); $path.AddArc($ox + $s - $d, $oy, $d, $d, 270, 90)
    $path.AddArc($ox + $s - $d, $oy + $s - $d, $d, $d, 0, 90); $path.AddArc($ox, $oy + $s - $d, $d, $d, 90, 90); $path.CloseFigure()
    $rect = New-Object System.Drawing.RectangleF $ox, $oy, $s, $s
    $brush = New-Object System.Drawing.Drawing2D.LinearGradientBrush $rect, ([System.Drawing.Color]::FromArgb(255, 99, 102, 241)), ([System.Drawing.Color]::FromArgb(255, 168, 85, 247)), 50.0
    $g.FillPath($brush, $path)
    $pen = New-Object System.Drawing.Pen ([System.Drawing.Color]::White), ([float]($s * 0.085))
    $pen.LineJoin = 'Round'; $pen.StartCap = 'Round'; $pen.EndCap = 'Round'
    $u = { param($x, $y) New-Object System.Drawing.PointF ([float]($ox + $s * $x)), ([float]($oy + $s * $y)) }
    $g.DrawLines($pen, [System.Drawing.PointF[]]@((& $u 0.20 0.68), (& $u 0.20 0.33), (& $u 0.355 0.52), (& $u 0.51 0.33), (& $u 0.51 0.68)))
    $g.DrawLine($pen, (& $u 0.72 0.33), (& $u 0.72 0.66))
    $g.DrawLines($pen, [System.Drawing.PointF[]]@((& $u 0.60 0.54), (& $u 0.72 0.68), (& $u 0.84 0.54)))
    $g.Dispose()
    $bmp.Save("$layout\Assets\$name", [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
}
Save-Logo 44 44 'Square44x44Logo.png' 0.92
Save-Logo 150 150 'Square150x150Logo.png' 0.62
Save-Logo 71 71 'Square71x71Logo.png' 0.7
Save-Logo 310 150 'Wide310x150Logo.png' 0.62
Save-Logo 310 310 'Square310x310Logo.png' 0.55
Save-Logo 50 50 'StoreLogo.png' 0.9
Save-Logo 256 256 'FileLogo.png' 0.86

$markdown = '.md', '.markdown', '.mdown', '.mkd', '.mkdn', '.mdwn'
$others = '.adoc', '.asciidoc', '.json', '.yaml', '.yml', '.toml', '.csv', '.tsv', '.mmd', '.mermaid', '.dot', '.gv', '.svg', '.txt', '.log', '.xml', '.html', '.htm', '.css', '.ini', '.cfg', '.conf', '.sh', '.bash', '.py', '.js', '.ts', '.jsx', '.tsx', '.rs', '.go', '.c', '.h', '.cpp', '.cs', '.java', '.kt', '.rb', '.php', '.sql', '.lua'
$types = { param($list) ($list | ForEach-Object { "            <uap:FileType>$_</uap:FileType>" }) -join "`n" }
$manifest = @"
<?xml version="1.0" encoding="utf-8"?>
<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
         xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"
         xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities"
         IgnorableNamespaces="uap rescap">
  <Identity Name="$IdentityName" Publisher="$Publisher" Version="$version4" ProcessorArchitecture="x64" />
  <Properties>
    <DisplayName>Inkdown</DisplayName>
    <PublisherDisplayName>$PublisherDisplayName</PublisherDisplayName>
    <Logo>Assets\StoreLogo.png</Logo>
  </Properties>
  <Dependencies>
    <TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.17763.0" MaxVersionTested="10.0.26100.0" />
  </Dependencies>
  <Resources>
    <Resource Language="en-us" />
  </Resources>
  <Applications>
    <Application Id="Inkdown" Executable="inkdown.exe" EntryPoint="Windows.FullTrustApplication">
      <uap:VisualElements DisplayName="Inkdown" Description="Markdown, diagram and data editor" BackgroundColor="transparent"
                          Square150x150Logo="Assets\Square150x150Logo.png" Square44x44Logo="Assets\Square44x44Logo.png">
        <uap:DefaultTile Wide310x150Logo="Assets\Wide310x150Logo.png" Square71x71Logo="Assets\Square71x71Logo.png" Square310x310Logo="Assets\Square310x310Logo.png" />
      </uap:VisualElements>
      <Extensions>
        <uap:Extension Category="windows.fileTypeAssociation">
          <uap:FileTypeAssociation Name="markdown">
            <uap:DisplayName>Markdown Document</uap:DisplayName>
            <uap:Logo>Assets\FileLogo.png</uap:Logo>
            <uap:SupportedFileTypes>
$(& $types $markdown)
            </uap:SupportedFileTypes>
          </uap:FileTypeAssociation>
        </uap:Extension>
        <uap:Extension Category="windows.fileTypeAssociation">
          <uap:FileTypeAssociation Name="documents">
            <uap:DisplayName>Inkdown Document</uap:DisplayName>
            <uap:Logo>Assets\FileLogo.png</uap:Logo>
            <uap:SupportedFileTypes>
$(& $types $others)
            </uap:SupportedFileTypes>
          </uap:FileTypeAssociation>
        </uap:Extension>
      </Extensions>
    </Application>
  </Applications>
  <Capabilities>
    <rescap:Capability Name="runFullTrust" />
  </Capabilities>
</Package>
"@
[System.IO.File]::WriteAllText("$layout\AppxManifest.xml", $manifest, (New-Object System.Text.UTF8Encoding $false))

$dist = Join-Path $root 'dist'
New-Item -ItemType Directory -Force $dist | Out-Null
$out = Join-Path $dist "Inkdown-$Version-x64.msix"
& $makeappx.FullName pack /d $layout /p $out /o | Select-Object -Last 2
if ($LASTEXITCODE -ne 0) { throw "makeappx failed ($LASTEXITCODE)" }
"MSIX: $out ({0:N1} MB)  identity=$IdentityName  publisher=$Publisher  version=$version4" -f ((Get-Item $out).Length / 1MB)
