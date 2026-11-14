# Captures the screenshots used in the README and the demo video.
#
#   pwsh tools\demo\capture.ps1                 # uses native\target\release\inkdown.exe
#   pwsh tools\demo\capture.ps1 -Exe dist\inkdown.exe
#
# The app is driven by its scripted test driver (no real mouse or keyboard) in an isolated
# profile, so your own settings are not touched. Screenshots go to docs\media\shots.
# Then run: python tools\demo\make-video.py
param([string]$Exe = "", [string]$Out = "")

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not $Exe) { $Exe = Join-Path $root "native\target\release\inkdown.exe" }
if (-not (Test-Path $Exe)) { throw "Build first: $Exe not found" }
if (-not $Out) { $Out = Join-Path $root "docs\media\shots" }
New-Item -ItemType Directory -Force $Out | Out-Null
Get-ChildItem $Out -Filter *.png | Remove-Item -Force

# work on a copy of the sample project, in a short neutral path
$work = Join-Path ([System.IO.Path]::GetTempPath()) "inkdown-demo"
Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $work | Out-Null
Copy-Item (Join-Path $PSScriptRoot "weather-station") $work -Recurse
$p = (Join-Path $work "weather-station").Replace('\', '/')
$s = $Out.Replace('\', '/')

function Run([string]$name, [string]$body) {
    $script = Join-Path $work "$name.txt"
    Set-Content $script $body
    Remove-Item -Recurse -Force (Join-Path $work "profile") -ErrorAction SilentlyContinue
    $env:INKDOWN_AUTOMATION = $script
    $env:INKDOWN_DATA_DIR = Join-Path $work "profile"
    $env:INKDOWN_WINDOW = "1280x840"
    $env:INKDOWN_NO_SESSION = "1"
    $env:INKDOWN_DIALOG_DIR = Join-Path $work "out"
    $proc = Start-Process -FilePath $Exe -PassThru
    if (-not $proc.WaitForExit(180000)) { $proc.Kill(); throw "$name timed out" }
    Write-Host "captured: $name"
}

# --- the tour page: reader, diagrams, the four modes, palette, themes
Run "tour" @"
wait 45
shot $s/01-read.png
move 640 450
scroll 0 -1250
wait 30
shot $s/02-math-diagrams.png
scroll 0 -1100
wait 30
shot $s/03-diagrams.png
scroll 0 -1100
wait 30
shot $s/04-charts.png
cmd mode write
wait 25
shot $s/05-write.png
cmd mode split
wait 30
shot $s/06-split.png
cmd mode code
wait 20
shot $s/07-code.png
key ctrl+shift+p
wait 6
type sort
wait 8
shot $s/08-palette.png
key escape
cmd mode read
wait 10
click 1197 23
wait 10
shot $s/09-export.png
key escape
cmd theme light
wait 15
shot $s/10-theme-light.png
cmd theme sepia
wait 15
shot $s/11-theme-sepia.png
cmd theme nord
wait 15
shot $s/12-theme-nord.png
cmd theme dracula
wait 15
shot $s/13-theme-dracula.png
quit
"@

# --- a project folder: code, multiple cursors, Goto Anything, data, split panes, search
Run "project" @"
wait 20
open $p
wait 30
open $p/src/main.rs
wait 20
shot $s/14-project.png
click 600 300
key ctrl+home
key down
key down
key down
key down
key down
key down
key down
key down
key down
key ctrl+right
key ctrl+right
key ctrl+right
key ctrl+d
key ctrl+d
key ctrl+d
key ctrl+d
wait 5
shot $s/15-multicursor.png
key escape
key ctrl+p
wait 6
type conf
wait 8
shot $s/16-goto.png
key enter
wait 30
shot $s/17-json.png
open $p/data/sales.csv
wait 25
shot $s/18-csv.png
open $p/data/readings.avro
wait 30
shot $s/18b-avro.png
open $p/docs/plan.md
wait 30
cmd mode split
wait 40
shot $s/19-markdown-split.png
cmd mode read
cmd layout_columns_2
wait 10
open $p/src/main.rs
wait 10
cmd move_to_group_2
wait 30
shot $s/20-two-panes.png
cmd layout_single
wait 10
cmd find_in_files
wait 6
key ctrl+a
type Reading
key enter
sleep 1500
wait 5
shot $s/21-find-in-files.png
quit
"@

# --- the diagram designer
Run "designer" @"
wait 20
open $p/docs/diagram.svg
cmd mode write
wait 15
click 66 176
wait 4
click 66 290
wait 4
click 66 215
wait 4
click 66 250
wait 10
drag 706 467 380 240
wait 20
drag 706 572 640 240
wait 20
drag 706 690 900 240
wait 20
drag 706 815 640 420
wait 20
click 300 600
wait 10
shot $s/22-designer.png
quit
"@

Get-ChildItem $Out -Filter *.png | ForEach-Object { '{0}  {1:N0} KB' -f $_.Name, ($_.Length / 1KB) }
