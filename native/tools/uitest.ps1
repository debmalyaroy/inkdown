# UI regression suite for the native app.
#
# Drives the real window through INKDOWN_AUTOMATION scripts (synthetic input, no real
# mouse or keyboard) in an isolated profile, then checks the state dumps and files.
#
#   pwsh tools\uitest.ps1                # debug build
#   pwsh tools\uitest.ps1 -Release       # release build
#   pwsh tools\uitest.ps1 -Only editor   # one group
#   pwsh tools/uitest.ps1 -Exe ./inkdown # a binary from somewhere else (Linux: see tools/linux-uitest)
#
# Runs on Windows and on Linux (there on a virtual display with software OpenGL, which is
# why the timing checks and the Windows installer checks are left out on Linux).
param([switch]$Release, [string]$Only = "", [string]$Out = "", [string]$Exe = "")

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$windows = $IsWindows -or $env:OS -eq "Windows_NT"
$exe = if ($Exe) { (Resolve-Path $Exe).Path } else { Join-Path $root (Join-Path "target" (Join-Path $(if ($Release) { "release" } else { "debug" }) $(if ($windows) { "inkdown.exe" } else { "inkdown" }))) }
if (-not (Test-Path $exe)) { throw "Build first: $exe not found" }
if (-not $Out) { $Out = Join-Path ([System.IO.Path]::GetTempPath()) "inkdown-uitest" }
Remove-Item -Recurse -Force $Out -ErrorAction SilentlyContinue
# every path below is written with forward slashes, which Windows accepts as well
$o = $Out.Replace('\', '/')
$w = "$o/work"
New-Item -ItemType Directory -Force "$o/work/proj/src", "$o/export" | Out-Null
$fixtures = $PSScriptRoot.Replace('\', '/') + "/fixtures"

# A screenshot as something with Width, Height and GetPixel(x, y).R/.G/.B, on any system.
function Open-Png([string]$file) {
    if ($windows) {
        return [System.Drawing.Bitmap]::new($file)
    }
    $raw = "$file.rgb"
    $size = (& python3 -c "import sys; from PIL import Image; im = Image.open(sys.argv[1]).convert('RGB'); open(sys.argv[2], 'wb').write(im.tobytes()); print(im.width, im.height)" $file $raw) -split ' '
    $bytes = [System.IO.File]::ReadAllBytes($raw)
    $img = [pscustomobject]@{ Width = [int]$size[0]; Height = [int]$size[1]; Bytes = $bytes }
    $img | Add-Member ScriptMethod GetPixel { param($x, $y) $i = ($y * $this.Width + $x) * 3; [pscustomobject]@{ R = $this.Bytes[$i]; G = $this.Bytes[$i + 1]; B = $this.Bytes[$i + 2] } }
    $img | Add-Member ScriptMethod Dispose { }
    return $img
}

# ---------------------------------------------------------------- fixtures
Set-Content "$o/work/a.rs" "fn alpha() {`n    let foo = 1;`n    let bar = foo + foo;`n}`n" -NoNewline
Set-Content "$o/work/list.txt" "one`ntwo`nthree`nfour`nfive`n" -NoNewline
Set-Content "$o/work/doc.md" "# Trip plan`n`nPack **light** and bring a *map*.`n`n- [x] Tickets`n- [ ] Hotel`n`n| Day | Place |`n| --- | --- |`n| 1 | Rome |`n`n``````mermaid`nflowchart LR`n  A --> B`n```````n`nMath `$a^2`$ here.`n" -NoNewline
Set-Content "$o/work/data.json" '{"name":"ink","items":[1,2,{"deep":true}],"n":null}' -NoNewline
Set-Content "$o/work/bad.json" "{`n  `"a`": 1,`n  oops`n}" -NoNewline
Set-Content "$o/work/t.csv" "name,qty`napple,3`npear,5`n" -NoNewline
Copy-Item (Join-Path $root "..\tools\demo\weather-station\data\readings.avro") "$o/work/readings.avro"
Set-Content "$o/work/d.svg" "" -NoNewline
# something for the build system to run: a batch file on Windows, a shell script elsewhere
$hello = if ($windows) { "hello.bat" } else { "hello.sh" }
Set-Content "$o/work/$hello" $(if ($windows) { "@echo hello from build`r`n" } else { "echo hello from build`n" }) -NoNewline
Set-Content "$o/work/save.txt" "start`n" -NoNewline
Set-Content "$o/work/auto.txt" "start`n" -NoNewline
Set-Content "$o/work/proj/src/main.rs" "fn main() {`n    helper_one();`n}`n" -NoNewline
Set-Content "$o/work/proj/src/util.rs" "pub fn helper_one() {}`npub fn helper_two() {}`n" -NoNewline
Set-Content "$o/work/proj/notes.md" "# Notes`n`nhelper_one is used in main.`n" -NoNewline
$big = [System.Text.StringBuilder]::new()
for ($i = 0; $i -lt 120000; $i++) { [void]$big.Append("fn item_$i(x: u32) -> u32 { x + $i } // line $i`n") }
[System.IO.File]::WriteAllText("$o/work/big.rs", $big.ToString())

$script:pass = 0; $script:fail = 0; $script:failed = @()
function Check([string]$name, [bool]$ok, [string]$detail = "") {
    if ($ok) { $script:pass++; Write-Host "  ok   $name" }
    else { $script:fail++; $script:failed += $name; Write-Host "  FAIL $name  $detail" -ForegroundColor Red }
}
function Run([string]$name, [string]$body, [switch]$Keep, [string[]]$Files = @()) {
    if (-not $Keep) { Remove-Item -Recurse -Force "$o/profile" -ErrorAction SilentlyContinue }
    Set-Content "$o/$name.txt" $body
    $env:INKDOWN_DATA_DIR = "$o/profile"
    $env:INKDOWN_AUTOMATION = "$o/$name.txt"
    $env:INKDOWN_WINDOW = "1280x840"
    $env:INKDOWN_DIALOG_DIR = "$o/export"
    $p = if ($Files.Count) { Start-Process -FilePath $exe -ArgumentList $Files -PassThru } else { Start-Process -FilePath $exe -PassThru }
    if (-not $p.WaitForExit(120000)) { $p.Kill(); Check "$name finished" $false "timed out" ; return $false }
    return $true
}
function J([string]$file) { Get-Content "$o/$file.json" -Raw | ConvertFrom-Json }
function Doc($j) { $j.docs[$j.active] }
# The updater checks are about the Windows installer, and frame times mean nothing when
# OpenGL is drawn in software, so those two groups run on Windows only.
# An argument that may hold spaces: quoted for Windows, as it is elsewhere.
function Arg([string]$value) { if ($windows) { "`"$value`"" } else { $value } }

function Want([string]$group) {
    if (-not $windows -and $group -in @("update", "perf")) { return $false }
    -not $Only -or $Only -eq $group
}

# ------------------------------------------------------------------ start
if (Want "start") {
    Write-Host "start"
    if (Run "start" "wait 40`ndump $o/start.json`nshot $o/start.png`nquit") {
        $j = J "start"; $d = Doc $j
        Check "welcome tour opens in Read mode" ($d.name -eq "Welcome.md" -and $d.mode -eq "read")
        Check "document outline is built" ($d.outline -ge 8) "outline=$($d.outline)"
        Check "document is laid out" ($d.preview_height -gt 2000) "h=$($d.preview_height)"
        Check "welcome tour is not marked unsaved" (-not $d.dirty)
        Check "first frame under 1.5 s" ($j.first_frame_ms -lt 1500) "$($j.first_frame_ms) ms"
        Check "screenshot written" ((Test-Path "$o/start.png") -and (Get-Item "$o/start.png").Length -gt 20000)
    }
}

# ----------------------------------------------------------------- editor
if (Want "editor") {
    Write-Host "editor"
    $s = @"
wait 10
open $w/a.rs
wait 5
click 400 160
key ctrl+end
type // tail
dump $o/ed1.json
key ctrl+z
key ctrl+home
key ctrl+alt+down
key ctrl+alt+down
type X
dump $o/ed2.json
key ctrl+z
key escape
key ctrl+home
key down
key ctrl+right
key ctrl+right
key ctrl+d
key ctrl+d
key ctrl+d
dump $o/ed3.json
type value
dump $o/ed4.json
key ctrl+z
key escape
key ctrl+home
key ctrl+shift+d
dump $o/ed5.json
key ctrl+z
key ctrl+shift+down
dump $o/ed6.json
key ctrl+z
key ctrl+/
dump $o/ed7.json
key ctrl+/
dump $o/ed8.json
key ctrl+f
wait 3
type foo
wait 3
dump $o/ed9.json
key escape
key ctrl+g
wait 3
type 3
key enter
wait 3
dump $o/ed10.json
key ctrl+home
key ctrl+shift+[
dump $o/ed11.json
key ctrl+shift+p
wait 3
type upper
wait 3
dump $o/ed12.json
key escape
open $w/list.txt
wait 5
key f9
dump $o/ed13.json
quit
"@
    if (Run "editor" $s) {
        $orig = "fn alpha() {`n    let foo = 1;`n    let bar = foo + foo;`n}`n"
        Check "typing at the end of the file" ((Doc (J "ed1")).text -eq ($orig + "// tail"))
        $d = Doc (J "ed2")
        Check "three cursors type on three lines" ($d.sels.Count -eq 3 -and $d.text.StartsWith("Xfn alpha() {`nX    let foo = 1;`nX    let bar")) $d.text
        $d = Doc (J "ed3")
        Check "Ctrl+D selects each occurrence of the word" ($d.sels.Count -eq 3) "sels=$($d.sels.Count)"
        Check "typing replaces all selected occurrences" ((Doc (J "ed4")).text -eq "fn alpha() {`n    let value = 1;`n    let bar = value + value;`n}`n")
        Check "duplicate line" ((Doc (J "ed5")).text.StartsWith("fn alpha() {`nfn alpha() {`n"))
        Check "move line down, undo restores" ((Doc (J "ed6")).text.StartsWith("    let foo = 1;`nfn alpha() {`n"))
        Check "toggle comment on" ((Doc (J "ed7")).text.StartsWith("// fn alpha() {"))
        Check "toggle comment off" ((Doc (J "ed8")).text -eq $orig)
        $d = Doc (J "ed9")
        Check "find highlights every match" ($d.find_open -and $d.find_matches -eq 3) "matches=$($d.find_matches)"
        $d = Doc (J "ed10")
        Check "go to line" ($d.sels[0][0] -eq 30) "caret=$($d.sels[0][0])"
        Check "fold a block" ((Doc (J "ed11")).folds -eq 1)
        $j = J "ed12"
        Check "command palette filters commands" ($j.picker.items[0] -like "*UPPER CASE*") "$($j.picker.items -join ';')"
        Check "sort lines" ((Doc (J "ed13")).text -eq "five`nfour`none`nthree`ntwo`n")
    }
}

# --------------------------------------------------------------- markdown
if (Want "markdown") {
    Write-Host "markdown"
    $s = @"
wait 10
open $w/doc.md
wait 30
dump $o/md1.json
cmd mode write
wait 15
key ctrl+end
key ctrl+home
key down
key down
key down
key down
key end
dump $o/md2.json
key enter
type Passport
dump $o/md3.json
key ctrl+home
key shift+end
key ctrl+b
dump $o/md4.json
cmd mode split
wait 20
shot $o/md-split.png
dump $o/md5.json
quit
"@
    if (Run "markdown" $s) {
        $d = Doc (J "md1")
        Check "Markdown opens in Read mode with an outline" ($d.mode -eq "read" -and $d.outline -eq 1)
        $d = Doc (J "md3")
        Check "Enter continues a task list in Write mode" ($d.text.Contains("- [x] Tickets`n- [ ] Passport`n- [ ] Hotel")) $d.text
        Check "Ctrl+B makes the selection bold" ((Doc (J "md4")).text.StartsWith("**# Trip plan**") -or (Doc (J "md4")).text.StartsWith("# **Trip plan**")) (Doc (J "md4")).text.Substring(0, 20)
        Check "Split mode shows the preview" ((Doc (J "md5")).preview_height -gt 200)
    }
}

# ------------------------------------------------------------------- data
if (Want "data") {
    Write-Host "data"
    $s = @"
wait 10
open $w/data.json
wait 20
dump $o/data1.json
cmd tool_json_format
wait 5
dump $o/data2.json
open $w/bad.json
wait 20
dump $o/data3.json
open $w/t.csv
wait 15
dump $o/data4.json
cmd tool_csv_to_table
wait 5
dump $o/data5.json
open $w/readings.avro
wait 20
dump $o/data6.json
type x
dump $o/data7.json
cmd tool_avro_to_json
wait 8
dump $o/data8.json
open $w/readings.avro
cmd tool_avro_schema
wait 8
dump $o/data9.json
open $w/readings.avro
cmd mode code
wait 5
cmd upper_case
dump $o/data10.json
cmd close
wait 5
dump $o/data11.json
quit
"@
    if (Run "data" $s) {
        $d = Doc (J "data1")
        Check "JSON opens in Split with a tree" ($d.mode -eq "split" -and $d.tree_rows -ge 4 -and -not $d.tree_error) "rows=$($d.tree_rows)"
        Check "format JSON" ((Doc (J "data2")).text.Contains("`n  `"name`": `"ink`","))
        $d = Doc (J "data3")
        Check "invalid JSON reports the line" ($d.tree_error -and $d.tree_error -like "*3*") "$($d.tree_error)"
        Check "CSV opens as a table" ((Doc (J "data4")).kind -eq "CSV")
        Check "CSV converts to a Markdown table" ((Doc (J "data5")).text.StartsWith("| name  | qty |"))
        $d = Doc (J "data6")
        $a = if ($d.text) { $d.text | ConvertFrom-Json } else { $null }
        Check "Avro data file opens as a JSON tree" ($d.kind -eq "JSON" -and $d.mode -eq "split" -and $d.tree_rows -ge 3 -and -not $d.tree_error) "kind=$($d.kind) rows=$($d.tree_rows) err=$($d.tree_error)"
        Check "Avro records, codec and schema are decoded" ($a.avro.records -eq 12 -and $a.avro.codec -eq "deflate" -and $a.records.Count -eq 12 -and $a.records[2].sensor -eq "roof" -and $a.records[2].kind -eq "WIND" -and $a.records[2].cost -eq "13.00" -and $a.schema.name -like "*Reading") "$($a.avro | ConvertTo-Json -Compress)"
        Check "Avro view is read-only" ((Doc (J "data6")).read_only -and (Doc (J "data7")).text -eq $d.text -and -not (Doc (J "data7")).dirty)
        $d = Doc (J "data8")
        Check "Avro records copy to an editable JSON document" ($d.name -eq "readings.json" -and -not $d.read_only -and $d.text.TrimStart().StartsWith("[") -and ($d.text | ConvertFrom-Json).Count -eq 12) "$($d.name)"
        $d = Doc (J "data10")
        Check "read-only document: view commands work, editing commands do not" ($d.name -eq "readings.avro" -and $d.mode -eq "code" -and $d.text -ceq (Doc (J "data6")).text)
        Check "read-only document can be closed from a command" (((J "data11").docs | Where-Object { $_.name -eq "readings.avro" }).Count -eq 0)
        $d = Doc (J "data9")
        Check "Avro schema copies to an .avsc document" ($d.name -eq "readings.avsc" -and $d.kind -eq "JSON" -and ($d.text | ConvertFrom-Json).fields.Count -eq 10) "$($d.name) $($d.kind)"
    }
}

# --------------------------------------------------------------- designer
if (Want "designer") {
    Write-Host "designer"
    $s = @"
wait 10
open $w/d.svg
cmd mode write
wait 10
click 66 176
wait 3
click 66 290
wait 6
dump $o/dz1.json
shot $o/designer.png
key ctrl+s
wait 5
dump $o/dz2.json
quit
"@
    if (Run "designer" $s) {
        $d = Doc (J "dz1")
        Check "blocks added in the designer become SVG" ($d.text.StartsWith("<svg xmlns") -and $d.text.Contains("inkdown-diagram:"))
        Check "designer drawing saves to disk" (-not (Doc (J "dz2")).dirty -and (Get-Content "$o/work/d.svg" -Raw).Contains("inkdown-diagram:"))
    }
}

# ------------------------------------------------------------------ files
if (Want "files") {
    Write-Host "files"
    $s = @"
wait 10
open $w/save.txt
wait 5
click 400 160
key ctrl+end
type saved line
dump $o/f1.json
key ctrl+s
wait 5
dump $o/f2.json
open $w/auto.txt
wait 5
cmd toggle_auto_save
key ctrl+end
type auto line
sleep 2500
dump $o/f3.json
cmd theme dracula
wait 5
dump $o/f4.json
cmd new_file
type draft text
wait 3
dump $o/f5.json
quit
"@
    if (Run "files" $s) {
        Check "edits mark the tab unsaved" (Doc (J "f1")).dirty
        Check "Ctrl+S writes the file" (-not (Doc (J "f2")).dirty -and (Get-Content "$o/work/save.txt" -Raw) -eq "start`nsaved line")
        Check "AutoSave writes without Ctrl+S" ((Get-Content "$o/work/auto.txt" -Raw) -eq "start`nauto line" -and -not (Doc (J "f3")).dirty)
        Check "theme switches" ((J "f4").theme -eq "dracula")
        Check "settings go to the isolated profile" ((Test-Path "$o/profile/config/settings.json") -and (Get-Content "$o/profile/config/settings.json" -Raw).Contains('"dracula"'))
        # second launch restores the session, including the unsaved draft
        if (Run "session" "wait 20`ndump $o/f6.json`nquit" -Keep) {
            $j = J "f6"
            Check "session restore reopens files" (($j.docs | Where-Object { $_.name -eq "save.txt" }).Count -eq 1) "$($j.docs.name -join ',')"
            Check "session restore keeps unsaved text" (($j.docs | Where-Object { $_.text -eq "draft text" }).Count -eq 1)
            Check "theme persists" ($j.theme -eq "dracula")
        }
    }
}

# ---------------------------------------------------------------- project
if (Want "project") {
    Write-Host "project"
    $s = @"
wait 10
open $w/proj
wait 30
key ctrl+p
wait 5
type util
wait 5
dump $o/p1.json
key enter
wait 10
dump $o/p2.json
cmd find_in_files
wait 5
type helper_one
key enter
sleep 1500
dump $o/p3.json
open $w/proj/src/main.rs
wait 5
key ctrl+home
key down
key ctrl+right
key f12
wait 15
dump $o/p4.json
open $w/$hello
wait 5
cmd build
sleep 2500
dump $o/p5.json
quit
"@
    if (Run "project" $s) {
        $j = J "p1"
        Check "Goto Anything lists project files" ($j.picker.items[0] -like "*util.rs") "$($j.picker.items -join ';')"
        Check "Goto Anything opens the file" ((Doc (J "p2")).name -eq "util.rs")
        Check "Find in Files finds every match" ((J "p3").search_results -eq 3) "results=$((J 'p3').search_results)"
        $d = Doc (J "p4")
        Check "Go to Definition jumps across files" ($d.name -eq "util.rs" -and $d.sels[0][0] -eq 7) "$($d.name) @ $($d.sels[0][0])"
        $j = J "p5"
        Check "build system runs and captures output" ($j.build_tail -like "*hello from build*") "$($j.build_tail)"
    }
}

# ----------------------------------------------------------------- export
if (Want "export") {
    Write-Host "export"
    $s = @"
wait 10
open $w/doc.md
wait 20
cmd export_html
cmd export_docx
cmd export_rtf
cmd export_pdf
sleep 6000
open $o/export/doc.pdf
wait 30
sleep 1500
cmd show_find
wait 5
type Trip plan
wait 8
dump $o/x-pdf.json
dump $o/x1.json
quit
"@
    if (Run "export" $s) {
        $h = "$o/export/doc.html"
        Check "HTML export" ((Test-Path $h) -and (Get-Content $h -Raw).Contains("<h1 id=`"trip-plan`">Trip plan</h1>"))
        Check "HTML embeds the diagram and math" ((Test-Path $h) -and ([regex]::Matches((Get-Content $h -Raw), "data:image/svg\+xml").Count -ge 2))
        Check "Word export" ((Test-Path "$o/export/doc.docx") -and (Get-Item "$o/export/doc.docx").Length -gt 5000)
        Check "RTF export" ((Test-Path "$o/export/doc.rtf") -and (Get-Content "$o/export/doc.rtf" -TotalCount 1).StartsWith("{\rtf1"))
        Check "PDF export" ((Test-Path "$o/export/doc.pdf") -and (Get-Item "$o/export/doc.pdf").Length -gt 3000)
        $x = Doc (J "x-pdf")
        Check "the exported PDF is written without a browser and holds real text" ($x.kind -eq "PDF" -and $x.pdf_pages -ge 1 -and $x.pdf_matches -ge 1 -and (J "x-pdf").launched.Count -eq 0) "pages=$($x.pdf_pages) matches=$($x.pdf_matches) launched=$((J 'x-pdf').launched -join ',')"
    }
}

# ----------------------------------------------------------------- drafts
# Unsaved work must survive however the app ends, and a second launch must not disturb it.
if (Want "drafts") {
    Write-Host "drafts"
    Set-Content "$o/work/kept.txt" "on disk`n" -NoNewline
    Set-Content "$o/work/gone.txt" "will be deleted`n" -NoNewline
    Set-Content "$o/work/later.md" "# Opened later`n" -NoNewline
    $s = @"
wait 10
cmd new_file
type first draft
cmd new_code_file
type second draft
open $w/kept.txt
wait 5
key ctrl+end
type EDITED
open $w/gone.txt
wait 5
key ctrl+end
type CHANGED
sleep 2600
wait 3
dump $o/dr1.json
crash
"@
    if (Run "drafts" $s) {
        Check "drafts are written to disk while the app runs" ((Get-ChildItem "$o/profile/config/drafts" -Filter *.json -ErrorAction SilentlyContinue).Count -eq 4) "$((Get-ChildItem "$o/profile/config/drafts" -ErrorAction SilentlyContinue).Count) files"
        Remove-Item "$o/work/gone.txt"
        if (Run "drafts2" "wait 25`ndump $o/dr2.json`ncmd select_tab_1`ncmd close`nwait 4`nkey escape`ndump $o/dr3.json`nquit" -Keep) {
            $j = J "dr2"; $names = $j.docs.name -join ","
            Check "after a crash, untitled drafts come back" ((($j.docs | Where-Object { $_.text -eq "first draft" }).Count -eq 1) -and (($j.docs | Where-Object { $_.text -eq "second draft" }).Count -eq 1)) $names
            $k = $j.docs | Where-Object { $_.name -eq "kept.txt" }
            Check "after a crash, unsaved edits to a file come back, and the file is untouched" ($k.text -eq "on disk`nEDITED" -and $k.dirty -and (Get-Content "$o/work/kept.txt" -Raw) -eq "on disk`n") "$($k.text)"
            Check "unsaved edits to a file that was deleted come back as a document" ((($j.docs | Where-Object { $_.text -eq "will be deleted`nCHANGED" }).Count -eq 1)) $names
            Check "the welcome tour does not reappear over restored work" (($j.docs | Where-Object { $_.name -eq "Welcome.md" }).Count -eq 0) $names
        }
        # a normal exit keeps them too, and what was closed on purpose stays closed
        if (Run "drafts3" "wait 25`ndump $o/dr4.json`nquit" -Keep) {
            $j = J "dr4"
            Check "drafts survive a normal exit as well" ((($j.docs | Where-Object { $_.text -eq "second draft" }).Count -eq 1) -and (($j.docs | Where-Object { $_.name -eq "kept.txt" -and $_.dirty }).Count -eq 1)) "$($j.docs.name -join ',')"
        }
    }
    # a second launch hands its file to the running window instead of opening another
    Remove-Item -Recurse -Force "$o/profile" -ErrorAction SilentlyContinue
    Set-Content "$o/inst.txt" "wait 10`ncmd new_file`ntype keep me`nsleep 9000`nwait 5`ndump $o/in1.json`nquit"
    $env:INKDOWN_DATA_DIR = "$o/profile"; $env:INKDOWN_WINDOW = "1280x840"; $env:INKDOWN_SINGLE_INSTANCE = "1"
    $env:INKDOWN_AUTOMATION = "$o/inst.txt"
    $first = Start-Process -FilePath $exe -PassThru
    Start-Sleep 4
    Remove-Item Env:INKDOWN_AUTOMATION
    $second = Start-Process -FilePath $exe -ArgumentList (Arg "$o/work/later.md") -PassThru
    $handed = $second.WaitForExit(8000)
    if (-not $handed) { $second.Kill() }
    if (-not $first.WaitForExit(60000)) { $first.Kill() }
    Remove-Item Env:INKDOWN_SINGLE_INSTANCE
    Check "a second launch exits after handing over its file" ($handed -and $second.ExitCode -eq 0) "exited=$handed"
    if (Test-Path "$o/in1.json") {
        $j = J "in1"
        Check "the running window opens that file and keeps its draft" ((($j.docs | Where-Object { $_.name -eq "later.md" }).Count -eq 1) -and (($j.docs | Where-Object { $_.text -eq "keep me" }).Count -eq 1)) "$($j.docs.name -join ',')"
    } else { Check "the running window opens that file and keeps its draft" $false "no dump" }
}

# ------------------------------------------------------------------ notes
if (Want "notes") {
    Write-Host "notes"
    Remove-Item -Recurse -Force "$o/profile", "$o/notes" -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force "$o/profile/config", "$o/notes/Projects" | Out-Null
    Set-Content "$o/profile/config/settings.json" ('{ "notes_folder": "' + "$o/notes" + '" }')
    Set-Content "$o/work/loose.md" "# Loose file`n`nSaved somewhere else.`n" -NoNewline
    $s = @"
wait 10
cmd new_file
type # Trip plan
key enter
key enter
type Pack light
cmd notes_file_to
wait 6
dump $o/nt1.json
cmd new_code_file
type call the bank / before 5?
cmd new_file
type ## Reading list
key enter
type - one
cmd notes_file_all
wait 6
dump $o/nt2.json
open $w/loose.md
wait 8
cmd notes_file
wait 5
dump $o/nt3.json
type proj
key enter
wait 8
dump $o/nt4.json
cmd notes_open
wait 15
dump $o/nt5.json
quit
"@
    if (Run "notes" $s -Keep) {
        $d = Doc (J "nt1")
        Check "a draft is filed in the Inbox as Markdown, named after its first line" ($d.name -eq "Trip plan.md" -and -not $d.dirty -and (Test-Path "$o/notes/Inbox/Trip plan.md") -and (Get-Content "$o/notes/Inbox/Trip plan.md" -Raw).StartsWith("# Trip plan")) "$($d.name) $($d.path)"
        $inbox = (Get-ChildItem "$o/notes/Inbox" -ErrorAction SilentlyContinue).Name -join ","
        Check "File All Drafts files every unsaved draft" ((Test-Path "$o/notes/Inbox/call the bank before 5.md") -and (Test-Path "$o/notes/Inbox/Reading list.md") -and (((J "nt2").docs | Where-Object { -not $_.path -and $_.name -ne "Welcome.md" }).Count -eq 0)) $inbox
        Check "the notebook picker lists Inbox, the notebooks and New notebook" ((((J "nt3").picker.items -join ";") -like "Inbox*Projects*New notebook*")) "$((J 'nt3').picker.items -join ';')"
        Check "a saved file moves into the chosen notebook" ((Test-Path "$o/notes/Projects/loose.md") -and -not (Test-Path "$o/work/loose.md") -and (Doc (J "nt4")).path -like "*Projects*loose.md") "$((Doc (J 'nt4')).path)"
        Check "Browse Notes opens the notes folder in the sidebar" ((J "nt5").folder -like "*notes" -and (J "nt5").sidebar) "$((J 'nt5').folder)"
        Check "filed notes leave no draft files behind" ((Get-ChildItem "$o/profile/config/drafts" -Filter *.json -ErrorAction SilentlyContinue).Count -eq 0)
    }
}

# -------------------------------------------------------------------- pdf
if (Want "pdf") {
    Write-Host "pdf"
    Copy-Item "$fixtures/sample.pdf" "$o/work/sample.pdf" -Force
    Copy-Item "$fixtures/sample.pdf" "$o/work/renamed.bin" -Force
    Set-Content "$o/work/broken.pdf" "%PDF-1.4 this is not really one" -NoNewline
    $before = (Get-FileHash "$o/work/sample.pdf").Hash
    $s = @"
wait 10
open $w/sample.pdf
wait 30
sleep 3000
wait 5
dump $o/pdf1.json
shot $o/pdf1.png
move 640 400
scroll 0 -1500
wait 10
dump $o/pdf2.json
type should not be typed
cmd save
wait 4
open $w/renamed.bin
wait 20
dump $o/pdf3.json
open $w/broken.pdf
wait 20
dump $o/pdf4.json
quit
"@
    if (Run "pdf" $s) {
        $d = Doc (J "pdf1")
        Check "a PDF opens as pages, read-only, in Read mode" ($d.kind -eq "PDF" -and $d.mode -eq "read" -and $d.pdf_pages -eq 3 -and $d.read_only -and -not $d.pdf_error) "$($d.kind) pages=$($d.pdf_pages) $($d.pdf_error)"
        $bmp = Open-Png "$o/pdf1.png"
        $white = 0; $blue = 0
        for ($x = 300; $x -lt $bmp.Width; $x += 7) { for ($y = 120; $y -lt $bmp.Height - 60; $y += 7) {
            $c = $bmp.GetPixel($x, $y)
            if ($c.R -gt 250 -and $c.G -gt 250 -and $c.B -gt 250) { $white++ }
            if ($c.B -gt 200 -and $c.R -lt 90 -and $c.G -lt 140) { $blue++ }
        } }
        $bmp.Dispose()
        Check "the page is drawn: white paper with its blue bar" ($white -gt 2000 -and $blue -gt 20) "white=$white blue=$blue"
        Check "scrolling moves to a later page" ((Doc (J "pdf2")).pdf_page -ge 2) "page=$((Doc (J 'pdf2')).pdf_page)"
        Check "the PDF file is never rewritten" ((Get-FileHash "$o/work/sample.pdf").Hash -eq $before)
        Check "a PDF under another name is recognised" ((Doc (J "pdf3")).kind -eq "PDF" -and (Doc (J "pdf3")).pdf_pages -eq 3) "$((Doc (J 'pdf3')).kind)"
        Check "a damaged PDF gives a message, not a crash" ([bool](Doc (J "pdf4")).pdf_error) "$((Doc (J 'pdf4')).pdf_error)"
    }
    # the text of a PDF: search, selection, and a file protected by a password
    Copy-Item "$fixtures/locked.pdf" "$o/work/locked.pdf" -Force
    $s = @"
wait 10
open $w/sample.pdf
wait 30
sleep 1200
wait 5
cmd show_find
wait 5
type PAGE
wait 5
key enter
wait 5
dump $o/pdf5.json
key enter
wait 8
dump $o/pdf6.json
key escape
wait 3
dump $o/pdf7.json
quit
"@
    if (Run "pdf-find" $s) {
        $a = Doc (J "pdf5"); $b = Doc (J "pdf6")
        Check "Find searches the text of every page of a PDF" ($a.pdf_text_pages -eq 3 -and $a.pdf_matches -eq 3 -and $a.pdf_match -eq 1) "text pages=$($a.pdf_text_pages) matches=$($a.pdf_matches) at=$($a.pdf_match)"
        Check "Enter moves to the next match, on the next page" ($b.pdf_match -eq 2 -and $b.pdf_page -eq 2) "match=$($b.pdf_match) page=$($b.pdf_page)"
        # drag across the title of the first page to select it
        $f = (Doc (J "pdf1")).pdf_first_page   # where the first page is before any scrolling
        if ($f) {
            $x0 = [int]($f[0] + 40 * $f[2]); $x1 = [int]($f[0] + 560 * $f[2]); $y = [int]($f[1] + 80 * $f[2])
            if (Run "pdf-select" "wait 10`nopen $w/sample.pdf`nwait 30`nsleep 1200`nwait 5`ndrag $x0 $y $x1 $y`nwait 12`ndump $o/pdf8.json`nquit") {
                Check "dragging over text in a PDF selects it" ((Doc (J "pdf8")).pdf_selection -eq "Inkdown sample, page 1") "$((Doc (J 'pdf8')).pdf_selection)"
            }
            # a double click on an empty part of the page zooms in, a second one zooms out again
            $bx = [int]($f[0] + 420 * $f[2]); $by = [int]($f[1] + 262 * $f[2])
            if (Run "pdf-zoom" "wait 10`nopen $w/sample.pdf`nwait 30`nsleep 1200`nwait 5`ndclick $bx $by`nwait 12`ndump $o/pdf12.json`nsleep 900`nwait 3`ndclick $bx $by`nwait 12`ndump $o/pdf13.json`nquit") {
                $z1 = (Doc (J "pdf12")).pdf_first_page[2]; $z2 = (Doc (J "pdf13")).pdf_first_page[2]
                Check "a double click on the page zooms in, and a second one back out" ($z1 -gt $f[2] * 1.9 -and [math]::Abs($z2 - $f[2]) -lt 0.02) "scale $($f[2]) -> $z1 -> $z2"
            }
        } else { Check "dragging over text in a PDF selects it" $false "no page position" }
    }
    $s = @"
wait 10
open $w/locked.pdf
wait 20
dump $o/pdf9.json
type wrong
key enter
wait 15
dump $o/pdf10.json
type inkdown
key enter
wait 30
sleep 800
wait 5
dump $o/pdf11.json
quit
"@
    if (Run "pdf-locked" $s) {
        Check "a protected PDF asks for its password" ((Doc (J "pdf9")).pdf_needs_password -and (Doc (J "pdf9")).pdf_pages -eq 0)
        Check "a wrong password is refused" ((Doc (J "pdf10")).pdf_needs_password)
        Check "the right password opens it" ((Doc (J "pdf11")).pdf_pages -eq 3 -and -not (Doc (J "pdf11")).pdf_needs_password -and (Doc (J "pdf11")).pdf_text_pages -eq 3) "pages=$((Doc (J 'pdf11')).pdf_pages)"
    }
}

# ---------------------------------------------------------- closing
# Closing a tab asks about unsaved changes. Closing the window does not, because the
# work comes back on the next start; it asks only about what could not be kept.
if (Want "closing") {
    Write-Host "closing"
    Set-Content "$o/work/c1.txt" "saved text`n" -NoNewline
    $s = @"
wait 10
open $w/c1.txt
wait 5
key ctrl+end
type EDIT
cmd close
wait 4
dump $o/cl1.json
key escape
wait 3
cmd new_file
type a draft
close_window
sleep 1500
dump $o/cl-still-open.json
quit
"@
    $closed = Run "closing" $s
    Check "closing a tab with unsaved changes asks first" ((J "cl1").dialog_kind -eq "ConfirmClose") "$((J 'cl1').dialog_kind)"
    Check "closing the window does not ask: the work is kept for the next start" ($closed -and -not (Test-Path "$o/cl-still-open.json") -and (Get-ChildItem "$o/profile/config/drafts" -Filter *.json).Count -eq 2) "drafts=$((Get-ChildItem "$o/profile/config/drafts" -Filter *.json -ErrorAction SilentlyContinue).Count)"
    if (Run "closing2" "wait 25`ndump $o/cl2.json`nquit" -Keep) {
        $j = J "cl2"
        Check "and it is there again afterwards" ((($j.docs | Where-Object { $_.text -eq "a draft" }).Count -eq 1) -and (($j.docs | Where-Object { $_.name -eq "c1.txt" -and $_.dirty -and $_.text -eq "saved text`nEDIT" }).Count -eq 1)) "$($j.docs.name -join ',')"
    }
    # with drafts switched off there is nothing to come back to, so the window asks
    Remove-Item -Recurse -Force "$o/profile" -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force "$o/profile/config" | Out-Null
    Set-Content "$o/profile/config/settings.json" '{ "hot_exit": false }'
    Remove-Item "$o/cl3.json", "$o/cl4.json" -ErrorAction SilentlyContinue
    $s = @"
wait 10
open $w/c1.txt
wait 5
key ctrl+end
type MORE
close_window
sleep 800
wait 3
dump $o/cl3.json
key escape
wait 3
dump $o/cl4.json
quit
"@
    if (Run "closing3" $s -Keep) {
        Check "the window asks when unsaved work could not be kept" ((J "cl3").dialog_kind -eq "ConfirmQuit") "$((J 'cl3').dialog_kind)"
        Check "Cancel keeps the window and the text" (-not (J "cl4").dialog -and (Doc (J "cl4")).text -eq "saved text`nEDITMORE".Replace("EDIT", "") -and (Get-Content "$o/work/c1.txt" -Raw) -eq "saved text`n") "$((Doc (J 'cl4')).text)"
    }
}

# ------------------------------------------------------------------- zoom
# Every Read view zooms: by pinch or Ctrl and the wheel (the `zoom` verb), and by the keyboard.
if (Want "zoom") {
    Write-Host "zoom"
    $s = @"
wait 10
open $w/doc.md
wait 5
cmd mode read
wait 30
move 640 400
dump $o/zm0.json
zoom 1.5
wait 20
dump $o/zm1.json
key ctrl+=
wait 20
dump $o/zm2.json
key ctrl+0
wait 20
dump $o/zm3.json
key ctrl+-
wait 20
dump $o/zm4.json
cmd mode code
wait 5
key ctrl+=
wait 5
dump $o/zm5.json
open $w/data.json
wait 5
cmd mode read
wait 20
move 640 400
zoom 2
wait 10
dump $o/zm6.json
open $w/t.csv
wait 5
cmd mode read
wait 20
move 640 400
zoom 0.5
wait 10
dump $o/zm7.json
open $w/sample.pdf
wait 30
sleep 1200
move 640 400
dump $o/zm8.json
zoom 1.5
wait 10
dump $o/zm9.json
key ctrl+0
wait 10
dump $o/zm10.json
quit
"@
    Copy-Item "$fixtures/sample.pdf" "$o/work/sample.pdf" -Force
    if (Run "zoom" $s) {
        $a = Doc (J "zm0"); $b = Doc (J "zm1")
        Check "a pinch (or Ctrl and the wheel) zooms the Markdown reader" ($a.read_zoom -eq 1 -and [math]::Abs($b.read_zoom - 1.5) -lt 0.01 -and $b.preview_height -gt $a.preview_height * 1.25) "zoom $($a.read_zoom) -> $($b.read_zoom), height $($a.preview_height) -> $($b.preview_height)"
        Check "Ctrl+= zooms further while reading, Ctrl+0 goes back to 100 %" ([math]::Abs((Doc (J "zm2")).read_zoom - 1.65) -lt 0.01 -and (Doc (J "zm3")).read_zoom -eq 1 -and [math]::Abs((Doc (J "zm3")).preview_height - $a.preview_height) -lt 2) "$((Doc (J 'zm2')).read_zoom) then $((Doc (J 'zm3')).read_zoom)"
        Check "Ctrl+- makes the page smaller than normal" ([math]::Abs((Doc (J "zm4")).read_zoom - 0.9) -lt 0.01) "$((Doc (J 'zm4')).read_zoom)"
        Check "in the editor the same keys still change the editor's text size, not the reader" ((J "zm5").font_size -eq ((J "zm0").font_size + 1) -and [math]::Abs((Doc (J "zm5")).read_zoom - 0.9) -lt 0.01) "font $((J 'zm0').font_size) -> $((J 'zm5').font_size)"
        Check "data trees zoom" ([math]::Abs((Doc (J "zm6")).read_zoom - 2) -lt 0.01) "$((Doc (J 'zm6')).read_zoom)"
        Check "tables zoom, down to half size" ([math]::Abs((Doc (J "zm7")).read_zoom - 0.5) -lt 0.01) "$((Doc (J 'zm7')).read_zoom)"
        $p0 = (Doc (J "zm8")).pdf_first_page[2]; $p1 = (Doc (J "zm9")).pdf_first_page[2]; $p2 = (Doc (J "zm10")).pdf_first_page[2]
        Check "PDF pages zoom by pinch, and Ctrl+0 fits the width again" ([math]::Abs($p1 - $p0 * 1.5) -lt 0.03 -and [math]::Abs($p2 - $p0) -lt 0.02) "scale $p0 -> $p1 -> $p2"
    }
}

# ------------------------------------------------------------------- logs
if (Want "logs") {
    Write-Host "logs"
    $lines = @(
        "2026-11-15T10:00:00.000Z INFO  [main] server started port=8080",
        "2026-11-15T10:00:01.000Z DEBUG [main] cache warmed entries=120",
        "2026-11-15T10:00:02.000Z WARN  [pool] connection pool nearly exhausted",
        "2026-11-15T10:00:03.000Z ERROR [worker-1] request failed id=7",
        "java.lang.IllegalStateException: boom",
        "`tat com.example.Main.run(Main.java:10)",
        "2026-11-15T10:00:04.000Z TRACE [main] tick"
    )
    Set-Content "$o/work/app.log" ($lines -join "`n") -NoNewline
    Set-Content "$o/work/service-output.txt" ($lines -join "`n") -NoNewline
    Set-Content "$o/work/plain.txt" "Just some INFO for you`nand an ERROR in prose`n" -NoNewline
    if (Run "logs" "open $w/app.log`nwait 25`ndump $o/lg1.json`nshot $o/lg1.png`nopen $w/service-output.txt`nwait 15`ndump $o/lg2.json`nopen $w/plain.txt`nwait 15`ndump $o/lg3.json`nquit") {
        Check "a .log file uses the log colouring" ((Doc (J "lg1")).syntax -eq "log") "$((Doc (J 'lg1')).syntax)"
        Check "a file that reads like a log is coloured as one, whatever its name" ((Doc (J "lg2")).syntax -eq "log") "$((Doc (J 'lg2')).syntax)"
        Check "ordinary text is not mistaken for a log" ((Doc (J "lg3")).syntax -ne "log") "$((Doc (J 'lg3')).syntax)"
        $bmp = Open-Png "$o/lg1.png"
        # count strongly red, amber and green pixels in the text area
        $red = 0; $amber = 0; $green = 0
        $k = $bmp.Width / 1280.0 / 1.25   # this suite was written on a display with 1.25 pixels per point
        for ($x = [int](100 * $k); $x -lt [int](900 * $k); $x += 2) { for ($y = [int](100 * $k); $y -lt [int](300 * $k); $y += 1) {
            $c = $bmp.GetPixel($x, $y)
            if ($c.R -gt $c.G + 90 -and $c.R -gt $c.B + 70) { $red++ }
            elseif ($c.R -gt $c.B + 90 -and $c.G -gt $c.B + 45 -and $c.R -gt $c.G + 15) { $amber++ }
            elseif ($c.G -gt $c.R + 45 -and $c.G -gt $c.B + 25) { $green++ }
        } }
        $bmp.Dispose()
        Check "errors are red, warnings amber and INFO green" ($red -gt 150 -and $amber -gt 60 -and $green -gt 10) "red=$red amber=$amber green=$green"
        Check "the first line of a stack trace is red like its entry" ($red -gt 420) "red=$red"
    }
}

# -------------------------------------------------------------- large file
# A file that is opened in the background, then grows while it is followed.
if (Want "bigfile") {
    Write-Host "bigfile"
    $line = "2026-11-15T10:00:00.000Z INFO  [worker-3] com.example.service.Handler - request completed status=200 path=/api/v1/orders/123 duration=45ms`n"
    $block = [System.Text.Encoding]::UTF8.GetBytes($line * 8000)
    $big = "$o/work/big.log"
    $fs = [System.IO.File]::Create($big)
    $count = 0
    while ($fs.Length -lt 30MB) { $fs.Write($block, 0, $block.Length); $count += 8000 }
    $fs.Close()
    Remove-Item "$o/bg1.json", "$o/bg2.json" -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force "$o/profile" -ErrorAction SilentlyContinue
    Set-Content "$o/bigfile.txt" "open $w/big.log`nwait 1`ndump $o/bg0.json`nsleep 4000`nwait 5`ncmd toggle_follow`nwait 3`ndump $o/bg1.json`nsleep 5000`nwait 5`ndump $o/bg2.json`nquit"
    $env:INKDOWN_DATA_DIR = "$o/profile"; $env:INKDOWN_WINDOW = "1280x840"; $env:INKDOWN_AUTOMATION = "$o/bigfile.txt"
    $p = Start-Process -FilePath $exe -PassThru
    $t0 = Get-Date
    while (-not (Test-Path "$o/bg1.json") -and ((Get-Date) - $t0).TotalSeconds -lt 60) { Start-Sleep -Milliseconds 200 }
    Start-Sleep -Milliseconds 300
    $peak = 0; try { $p.Refresh(); $peak = $p.PeakWorkingSet64 / 1MB } catch {}
    Add-Content -Path $big -Value "2026-11-15T11:00:00.000Z ERROR [worker-1] appended one`n2026-11-15T11:00:01.000Z WARN  [worker-1] appended two`n" -NoNewline
    if (-not $p.WaitForExit(60000)) { $p.Kill() }
    if ((Test-Path "$o/bg1.json") -and (Test-Path "$o/bg2.json")) {
        $a = Doc (J "bg0"); $b = Doc (J "bg1"); $c = Doc (J "bg2")
        Check "a large file gets its tab at once and is read in the background" ($a.name -eq "big.log" -and $b.lines -eq ($count + 1) -and -not $b.live.loading -and -not $b.read_only -and -not $b.dirty) "first: loading=$($a.live.loading) lines=$($a.lines); then lines=$($b.lines) of $($count + 1)"
        Check "it is recognised and coloured as a log" ($b.syntax -eq "log") "$($b.syntax)"
        if ($windows) { Check "reading it takes far less memory than three times its size" ($peak -gt 0 -and $peak -lt 30 * 2.2 + 130) ("peak {0:N0} MB for a 30 MB file" -f $peak) }
        Check "lines added to a followed file appear, without a reload" ($c.lines -eq ($count + 3) -and $c.live.updates -eq ($b.live.updates + 1) -and -not $c.dirty) "lines=$($c.lines) updates=$($b.live.updates)->$($c.live.updates)"
        Check "while following, the caret stays at the end" ($c.sels[0][1] -eq $c.chars -and $c.live.follow) "caret=$($c.sels[0][1]) chars=$($c.chars)"
    } else { Check "large file run produced its dumps" $false "missing" }
    Remove-Item $big -ErrorAction SilentlyContinue
}

# -------------------------------------------------------------------- web
# Documents from a web address, served by a test server on 127.0.0.1.
if (Want "web") {
    Write-Host "web"
    # "python" is the usual name on Windows, "python3" elsewhere
    $py = (Get-Command $(if ($windows) { "python", "python3" } else { "python3", "python" }) -ErrorAction SilentlyContinue | Select-Object -First 1).Source
    if (-not $py) { Write-Host "  skipped: python is needed for the test web server" }
    else {
        foreach ($mode in @("range", "plain")) {
            Remove-Item -Recurse -Force "$o/web", "$o/profile" -ErrorAction SilentlyContinue
            New-Item -ItemType Directory -Force "$o/web", "$o/profile/config" | Out-Null
            Set-Content "$o/profile/config/settings.json" '{ "live_refresh_seconds": 1 }'
            Set-Content "$o/web/app.log" "2026-11-15T10:00:00.000Z INFO  [main] started`n2026-11-15T10:00:01.000Z WARN  [main] slow`n" -NoNewline
            Set-Content "$o/web/README.md" "# Served`n`nFrom the test server.`n" -NoNewline
            Remove-Item "$o/port.txt", "$o/wb1.json" -ErrorAction SilentlyContinue
            $srvArgs = @((Arg "$fixtures/range_server.py"), (Arg "$o/web"), (Arg "$o/port.txt"))
            if ($mode -eq "plain") { $srvArgs += "--no-range" }
            if ($windows) {
                $srv = Start-Process -FilePath $py -ArgumentList $srvArgs -PassThru -WindowStyle Hidden
            } else {
                # through the shell, detached: the server ends by itself when its port file is removed
                $srv = $null
                $extra = if ($mode -eq "plain") { "--no-range" } else { "" }
                # give a server from the round before time to notice that it should stop
                Start-Sleep -Milliseconds 1500
                & /bin/sh -c "nohup '$py' '$fixtures/range_server.py' '$o/web' '$o/port.txt' $extra > '$o/server-out.txt' 2> '$o/server-err.txt' < /dev/null &"
            }
            # the first start of Python on a freshly made machine can take a long time
            $t0 = Get-Date
            while (-not (Test-Path "$o/port.txt") -and ((Get-Date) - $t0).TotalSeconds -lt 60) { Start-Sleep -Milliseconds 100 }
            Start-Sleep -Milliseconds 200
            if (-not (Test-Path "$o/port.txt")) {
                Check "[$mode] the test web server starts" $false "$py $($srvArgs -join ' ') :: $(Get-Content "$o/server-err.txt" -Raw -ErrorAction SilentlyContinue) :: $(Get-Content "$o/server-out.txt" -Raw -ErrorAction SilentlyContinue)"
                if ($srv -and -not $srv.HasExited) { $srv.Kill() }
                continue
            }
            $port = (Get-Content "$o/port.txt" -Raw).Trim()
            $u = "http://127.0.0.1:$port"
            Set-Content "$o/web.txt" "cmd open_url $u/app.log`nsleep 2500`nwait 5`ndump $o/wb1.json`nsleep 5000`nwait 5`ndump $o/wb2.json`ntype typed`nwait 3`ncmd open_url $u/README.md`nsleep 2000`nwait 5`ndump $o/wb3.json`ncmd open_url $u/missing.txt`nsleep 2000`nwait 5`ndump $o/wb4.json`ncmd open_url $u/stream.log`nsleep 2600`nwait 5`ndump $o/wb5.json`nsleep 2600`nwait 5`ndump $o/wb6.json`ncmd open_url $u/stream.log?n=3`nsleep 9000`nwait 5`ndump $o/wb7.json`nquit"
            $env:INKDOWN_DATA_DIR = "$o/profile"; $env:INKDOWN_WINDOW = "1280x840"; $env:INKDOWN_AUTOMATION = "$o/web.txt"
            $p = Start-Process -FilePath $exe -PassThru
            $t0 = Get-Date
            while (-not (Test-Path "$o/wb1.json") -and ((Get-Date) - $t0).TotalSeconds -lt 60) { Start-Sleep -Milliseconds 200 }
            Add-Content -Path "$o/web/app.log" -Value "2026-11-15T10:00:02.000Z ERROR [main] added on the server`n" -NoNewline
            if (-not $p.WaitForExit(60000)) { $p.Kill() }
            Remove-Item "$o/port.txt" -ErrorAction SilentlyContinue
            if ($srv -and -not $srv.WaitForExit(5000)) { $srv.Kill() }
            if (-not $srv) { Start-Sleep -Milliseconds 800 }
            $a = Doc (J "wb1"); $b = (J "wb2").docs | Where-Object { $_.name -eq "app.log" }
            Check "[$mode] a web address opens as a read-only document, coloured as a log and followed" ($a.name -eq "app.log" -and $a.live.url -eq "$u/app.log" -and $a.read_only -and $a.syntax -eq "log" -and $a.live.follow -and $a.lines -eq 3 -and -not $a.live.error) "$($a.name) lines=$($a.lines) syntax=$($a.syntax) err=$($a.live.error)"
            Check "[$mode] lines added on the server appear by themselves" ($b.text -like "*added on the server*" -and $b.lines -eq 4 -and $b.sels[0][1] -eq $b.chars) "lines=$($b.lines) updates=$($b.live.updates)"
            $c = (J "wb3").docs | Where-Object { $_.name -eq "app.log" }
            Check "[$mode] the document cannot be edited by accident" ($c.text -notlike "*typed*" -and -not $c.dirty)
            $m = Doc (J "wb3")
            Check "[$mode] Markdown from a web address opens in Read mode" ($m.name -eq "README.md" -and $m.kind -eq "Markdown" -and $m.mode -eq "read" -and $m.text -like "# Served*") "$($m.name) $($m.kind) $($m.mode)"
            $e = J "wb4"
            Check "[$mode] an address with nothing behind it says so" ((($e.toasts -join " ") -like "*no document*") -or ((Doc $e).live.error -like "*no document*")) "$($e.toasts -join ' | ')"
            $s1 = Doc (J "wb5"); $s2 = Doc (J "wb6")
            Check "[$mode] a connection the server keeps open is shown as it arrives" ($s1.name -eq "stream.log" -and $s1.live.streaming -and $s1.lines -ge 5 -and $s1.syntax -eq "log" -and $s1.text -like "*connected*tick 0 costs €5*") "streaming=$($s1.live.streaming) lines=$($s1.lines)"
            Check "[$mode] the stream keeps adding lines, whole characters only, and the view follows" ($s2.lines -ge ($s1.lines + 4) -and $s2.live.streaming -and $s2.text -notmatch "\uFFFD" -and $s2.text -notmatch "`r" -and $s2.sels[0][1] -eq $s2.chars -and ($s2.text -split "connected").Count -eq 2) "lines $($s1.lines) -> $($s2.lines)"
            $s3 = Doc (J "wb7")
            Check "[$mode] a stream the server ends and replays on reconnecting is not shown twice" ($s3.lines -eq 6 -and ($s3.text -split "connected").Count -eq 2 -and ($s3.text -split "tick 2 costs").Count -eq 2 -and -not $s3.live.error) "lines=$($s3.lines) streaming=$($s3.live.streaming)"
            Check "[$mode] documents from the web leave no draft files" ((Get-ChildItem "$o/profile/config/drafts" -Filter *.json -ErrorAction SilentlyContinue).Count -eq 0)
        }
    }
}

# ------------------------------------------------------------------ menus
# Every item of the main menu, its submenus and the toolbar, by command and by click.
if (Want "menus") {
    Write-Host "menus"
    Set-Content "$o/work/menu.md" "# Menu test`n`nSome ``code`` and a list:`n`n1. one`n1. two`n`n| a | b |`n|---|---|`n| 1 | 22222 |`n`n## Section`n`nText.`n" -NoNewline
    Set-Content "$o/work/menu2.txt" "second`n" -NoNewline
    Set-Content "$o/work/flow.mmd" "flowchart LR`n  A --> B`n" -NoNewline
    $env:INKDOWN_DIALOG_OPEN = "$o/work/menu2.txt"
    $env:INKDOWN_DIALOG_FOLDER = "$o/work/proj"
    $env:INKDOWN_UPDATE_URL = "http://127.0.0.1:9/none.json"   # nothing listens here: the check fails at once
    $s = @"
wait 10
open $w/menu.md
wait 15
dump $o/mn0.json
cmd new_file
wait 4
dump $o/mn1.json
cmd close
wait 3
cmd new_from_template
wait 5
dump $o/mn2.json
key down
key enter
wait 6
dump $o/mn3.json
cmd select_tab_1
wait 3
cmd open_file
wait 8
dump $o/mn4.json
cmd close
wait 3
cmd open_folder
wait 20
dump $o/mn5.json
cmd close_folder
cmd toggle_sidebar
wait 3
cmd open_recent
wait 5
dump $o/mn6.json
key escape
open $w/menu.md
wait 8
cmd mode code
wait 5
key ctrl+end
type x
cmd save_as
wait 8
dump $o/mn7.json
cmd export_copy_html
wait 4
dump $o/mn8.json
cmd export_copy_rich
wait 4
dump $o/mn9.json
cmd export_gdoc
wait 6
dump $o/mn10.json
key escape
cmd tool_word_count
wait 4
dump $o/mn11.json
key escape
cmd tool_format_tables
wait 4
dump $o/mn12.json
cmd tool_renumber
wait 4
dump $o/mn13.json
key ctrl+home
cmd tool_toc
wait 4
dump $o/mn14.json
key ctrl+end
type \nhello world
key shift+home
cmd tool_base64_encode
wait 3
dump $o/mn15.json
key end
key shift+home
cmd tool_base64_decode
wait 3
dump $o/mn16.json
key end
key shift+home
cmd tool_url_encode
wait 3
dump $o/mn17.json
key end
key shift+home
cmd tool_url_decode
wait 3
dump $o/mn18.json
key end
type \n
cmd tool_insert_date
type \n
cmd tool_insert_uuid
wait 3
dump $o/mn19.json
cmd tool_set_default
wait 3
dump $o/mn20.json
cmd select_theme
wait 5
dump $o/mn21.json
key escape
cmd increase_font_size
cmd increase_font_size
cmd decrease_font_size
cmd toggle_line_numbers
cmd toggle_minimap
cmd toggle_word_wrap
wait 3
dump $o/mn22.json
cmd toggle_line_numbers
cmd toggle_minimap
cmd toggle_word_wrap
cmd reset_font_size
cmd open_settings
wait 8
dump $o/mn23.json
cmd close
cmd open_keybindings
wait 8
dump $o/mn24.json
cmd close
cmd open_snippets
wait 8
dump $o/mn25.json
cmd close
cmd integrations
wait 4
dump $o/mn26.json
key escape
cmd command_palette
wait 4
dump $o/mn27.json
key escape
cmd show_shortcuts
wait 4
dump $o/mn28.json
key escape
cmd about
wait 4
dump $o/mn29.json
key escape
open $w/menu2.txt
wait 5
cmd help_docs
cmd help_release_notes
cmd help_report_issue
cmd new_window
cmd reveal_in_explorer
open $w/doc.md
wait 10
cmd print
wait 4
dump $o/mn30.json
cmd check_for_updates
sleep 1500
wait 3
dump $o/mn31.json
key escape
cmd welcome
wait 15
dump $o/mn32.json
cmd close
open $w/flow.mmd
wait 20
cmd export_svg
cmd export_png
sleep 2500
wait 3
dump $o/mn33.json
quit
"@
    if (Run "menus" $s) {
        $n0 = (J "mn0").docs.Count
        Check "File > New Document" ((J "mn1").docs.Count -eq $n0 + 1 -and (Doc (J "mn1")).name -like "Untitled-*")
        Check "File > New from Template lists templates" ((J "mn2").picker.items.Count -ge 3) "$((J 'mn2').picker.items -join ';')"
        Check "choosing a template creates a filled document" ((J "mn3").docs.Count -ge $n0 + 1 -and (Doc (J "mn3")).chars -gt 20) "chars=$((Doc (J 'mn3')).chars)"
        Check "File > Open opens the chosen file" ((Doc (J "mn4")).name -eq "menu2.txt")
        Check "File > Open Folder shows the folder" ((J "mn5").folder -like "*proj" -and (J "mn5").sidebar)
        Check "File > Open Recent lists recent files" ((J "mn6").picker.items.Count -ge 1 -and (((J "mn6").picker.items -join ";") -like "*menu*")) "$((J 'mn6').picker.items -join ';')"
        Check "File > Save As writes the file" ((Test-Path "$o/export/menu.md") -and -not (Doc (J "mn7")).dirty) "$((Doc (J 'mn7')).path)"
        Check "Export > Copy as HTML" (((J "mn8").launched -join ";") -like "*copy:<h1*") "$((J 'mn8').launched -join ';')"
        Check "Export > Copy as Rich Text" (((J "mn9").launched -join ";") -like "*copy:Menu test*" -and ((J "mn9").toasts -join ";") -notlike "*Could not copy*") "$((J 'mn9').toasts -join ';')"
        Check "Export > Google Docs asks to connect first" ((J "mn10").dialog -or (((J "mn10").toasts -join ";") -match "Google|connect|Integrations")) "$((J 'mn10').dialog_kind) $((J 'mn10').toasts -join ';')"
        Check "Tools > Word Count reports" ((J "mn11").dialog -or (((J "mn11").toasts -join ";") -match "word")) "$((J 'mn11').dialog_kind) $((J 'mn11').toasts -join ';')"
        Check "Tools > Align Markdown Tables" ((Doc (J "mn12")).text.Contains("| a   | b     |"))
        Check "Tools > Renumber Ordered Lists" ((Doc (J "mn13")).text.Contains("1. one`n2. two"))
        Check "Tools > Insert Table of Contents" ((Doc (J "mn14")).text -match "\[Section\]\(#section\)")
        Check "Tools > Base64 Encode" ((Doc (J "mn15")).text.TrimEnd().EndsWith("aGVsbG8gd29ybGQ="))
        Check "Tools > Base64 Decode" ((Doc (J "mn16")).text.TrimEnd().EndsWith("hello world"))
        Check "Tools > URL Encode" ((Doc (J "mn17")).text.TrimEnd().EndsWith("hello%20world"))
        Check "Tools > URL Decode" ((Doc (J "mn18")).text.TrimEnd().EndsWith("hello world"))
        $t = (Doc (J "mn19")).text
        Check "Tools > Insert Date and Insert UUID" ($t -match "\d{4}-\d{2}-\d{2}" -and $t -match "[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}")
        if ($windows) { Check "Tools > Make Inkdown the Default opens Windows Settings" (((J "mn20").launched -join ";") -like "*ms-settings:defaultapps*") "$((J 'mn20').launched -join ';')" }
        Check "Preferences > Theme lists the themes" ((J "mn21").picker.items.Count -ge 9) "$((J 'mn21').picker.items.Count)"
        $j = J "mn22"; $b = J "mn0"
        Check "Preferences > Larger / Smaller Text" ($j.font_size -eq $b.font_size + 1) "$($j.font_size)"
        Check "Preferences > Line Numbers, Minimap, Word Wrap toggle" ($j.line_numbers -ne $b.line_numbers -and $j.minimap -ne $b.minimap -and $null -ne (Doc $j).wrap) "$($j.line_numbers) $($j.minimap) wrap=$((Doc $j).wrap)"
        Check "Preferences > Settings opens settings.json" ((Doc (J "mn23")).name -eq "settings.json" -and (Doc (J "mn23")).text.Contains('"theme"'))
        Check "Preferences > Key Bindings opens keybindings.json" ((Doc (J "mn24")).name -eq "keybindings.json")
        Check "Preferences > Snippets opens snippets.json" ((Doc (J "mn25")).name -eq "snippets.json")
        Check "Integrations opens its dialog" ((J "mn26").dialog_kind -eq "Integrations") "$((J 'mn26').dialog_kind)"
        Check "Command Palette opens" ((J "mn27").picker.items.Count -ge 10)
        Check "Help > Keyboard Shortcuts" ((J "mn28").dialog_kind -eq "Shortcuts") "$((J 'mn28').dialog_kind)"
        Check "Help > About Inkdown" ((J "mn29").dialog_kind -eq "About") "$((J 'mn29').dialog_kind)"
        $l = (J "mn30").launched -join ";"
        Check "Help > Documentation, Release Notes, Report an Issue open their pages" ($l -like "*inkdown#readme*" -and $l -like "*/releases/latest*" -and $l -like "*/issues*") $l
        Check "New Window, Show in File Manager and Print reach the system" ($l -like "*new-window*" -and $l -like "*reveal:*menu2.txt*" -and $l -like "*-print.html*") $l
        Check "Help > Check for Updates opens the update dialog" ((J "mn31").dialog_kind -eq "Update" -and (J "mn31").update -in "checking", "failed") "$((J 'mn31').dialog_kind) $((J 'mn31').update)"
        Check "Help > Welcome Tour" ((Doc (J "mn32")).name -eq "Welcome.md")
        Check "Export > Diagram as SVG and PNG" ((Test-Path "$o/export/flow.svg") -and (Test-Path "$o/export/flow.png") -and (Get-Item "$o/export/flow.png").Length -gt 500) "$((J 'mn33').toasts -join ';')"
    }
    # the toolbar and the menus themselves, by clicking them
    $s = @"
wait 10
open $w/doc.md
wait 15
click 637 23
wait 6
dump $o/tb1.json
click 714 23
wait 6
dump $o/tb2.json
click 792 23
wait 6
dump $o/tb3.json
click 558 23
wait 6
dump $o/tb4.json
click 61 23
wait 6
dump $o/tb5.json
click 61 23
wait 4
click 166 23
wait 4
dump $o/tb6.json
click 166 23
click 1138 23
wait 6
dump $o/tb7.json
key escape
click 1254 23
wait 6
dump $o/tb8.json
key escape
click 22 23
wait 8
shot $o/menu-open.png
dump $o/tb9.json
key escape
quit
"@
    if (Run "toolbar" $s) {
        $fullWidth = (J "tb1").window[0] -ge 1270
        if (-not $fullWidth) { Write-Host "  note: the window is $((J 'tb1').window[0]) points wide here, so three toolbar clicks at fixed places are left out" }
        if ($fullWidth) { Check "toolbar: Write, Split, Code and Read buttons" ((Doc (J "tb1")).mode -eq "write" -and (Doc (J "tb2")).mode -eq "split" -and (Doc (J "tb3")).mode -eq "code" -and (Doc (J "tb4")).mode -eq "read") "$((Doc (J 'tb1')).mode) $((Doc (J 'tb2')).mode) $((Doc (J 'tb3')).mode) $((Doc (J 'tb4')).mode)" }
        Check "toolbar: sidebar button" ((J "tb5").sidebar)
        Check "toolbar: AutoSave switch" ((J "tb6").auto_save)
        if ($fullWidth) { Check "toolbar: search button opens Goto Anything" ($null -ne (J "tb7").picker) }
        if ($fullWidth) { Check "toolbar: theme button" ((J "tb8").theme -ne (J "tb1").theme -or $null -ne (J "tb8").picker) "$((J 'tb8').theme)" }
        Check "menu button opens the main menu" ((Test-Path "$o/menu-open.png"))
    }
    Remove-Item Env:INKDOWN_DIALOG_OPEN, Env:INKDOWN_DIALOG_FOLDER, Env:INKDOWN_UPDATE_URL -ErrorAction SilentlyContinue
}

# ----------------------------------------------------------------- update
# A tiny local web server stands in for GitHub Releases (127.0.0.1 only, random port).
if (Want "update") {
    Write-Host "update"
    $site = "$o/site"
    New-Item -ItemType Directory -Force $site | Out-Null
    $probe = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
    $probe.Start(); $port = $probe.LocalEndpoint.Port; $probe.Stop()
    $base = "http://127.0.0.1:$port"
    $server = Start-Job -ArgumentList $site, $port -ScriptBlock {
        param($dir, $port)
        $l = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $port)
        $l.Start()
        while ($true) {
            $c = $l.AcceptTcpClient()
            try {
                $s = $c.GetStream()
                $r = [System.IO.StreamReader]::new($s)
                $line = $r.ReadLine()
                while (($h = $r.ReadLine())) { }
                $name = ($line -split ' ')[1].TrimStart('/')
                $file = Join-Path $dir $name
                if ($name -and (Test-Path $file -PathType Leaf)) {
                    $body = [System.IO.File]::ReadAllBytes($file)
                    $head = [System.Text.Encoding]::ASCII.GetBytes("HTTP/1.1 200 OK`r`nContent-Length: $($body.Length)`r`nConnection: close`r`n`r`n")
                    $s.Write($head, 0, $head.Length); $s.Write($body, 0, $body.Length)
                } else {
                    $head = [System.Text.Encoding]::ASCII.GetBytes("HTTP/1.1 404 Not Found`r`nContent-Length: 0`r`nConnection: close`r`n`r`n")
                    $s.Write($head, 0, $head.Length)
                }
                $s.Flush()
            } catch { } finally { $c.Close() }
        }
    }
    Start-Sleep -Milliseconds 800
    function Publish([string]$version, [string]$sum) {
        [System.IO.File]::WriteAllBytes("$site\Inkdown-Setup.exe", [System.Text.Encoding]::ASCII.GetBytes("not a real installer $version"))
        if (-not $sum) { $sum = (Get-FileHash "$site\Inkdown-Setup.exe" -Algorithm SHA256).Hash.ToLower() }
        Set-Content "$site\SHA256SUMS.txt" "$sum  Inkdown-Setup.exe`n" -NoNewline
        $rel = @{ tag_name = "v$version"; html_url = "$base/notes"; assets = @(
                @{ name = "Inkdown-Setup.exe"; browser_download_url = "$base/Inkdown-Setup.exe" },
                @{ name = "SHA256SUMS.txt"; browser_download_url = "$base/SHA256SUMS.txt" }) }
        Set-Content "$site\latest.json" ($rel | ConvertTo-Json -Depth 5)
    }
    $got = Join-Path ([System.IO.Path]::GetTempPath()) "inkdown-update\Inkdown-Setup.exe"
    try {
        $env:INKDOWN_UPDATE_URL = "$base/latest.json"
        $steps = "wait 10`ncmd check_for_updates`nsleep 2500`nwait 3`ndump $o/NAME1.json`ncmd update_download`nsleep 2500`nwait 3`ndump $o/NAME2.json`nquit"

        Remove-Item $got -ErrorAction SilentlyContinue
        Publish "99.0.0" ""
        if (Run "up" $steps.Replace("NAME", "up")) {
            $j = J "up1"
            Check "a newer release is found and offered" ($j.update -eq "available" -and $j.update_version -eq "99.0.0" -and $j.dialog) "$($j.update) $($j.update_version)"
            Check "the installer is downloaded and its checksum verified" ((J "up2").update -eq "ready" -and (Test-Path $got) -and (Get-FileHash $got).Hash -eq (Get-FileHash "$site\Inkdown-Setup.exe").Hash) "$((J 'up2').update)"
        }
        Remove-Item $got -ErrorAction SilentlyContinue
        Publish "99.0.0" ("0" * 64)
        if (Run "upbad" $steps.Replace("NAME", "upbad")) {
            Check "a download with the wrong checksum is rejected and not saved" ((J "upbad2").update -eq "failed" -and -not (Test-Path $got)) "$((J 'upbad2').update)"
        }
        Publish "0.0.1" ""
        if (Run "upold" $steps.Replace("NAME", "upold")) {
            Check "an older release is not offered" ((J "upold1").update -eq "up-to-date") "$((J 'upold1').update)"
        }
        $env:INKDOWN_UPDATE_URL = "$base/missing.json"
        if (Run "upnone" $steps.Replace("NAME", "upnone")) {
            Check "a failed check is reported, not hidden" ((J "upnone1").update -eq "failed" -and (J "upnone1").dialog) "$((J 'upnone1').update)"
        }
        Remove-Item Env:INKDOWN_UPDATE_URL
        if (Run "upoff" "wait 10`nsleep 4500`nwait 3`ndump $o/upoff1.json`nquit") {
            Check "test runs never contact the real update server" ((J "upoff1").update -eq "idle") "$((J 'upoff1').update)"
        }
    } finally {
        Remove-Item Env:INKDOWN_UPDATE_URL -ErrorAction SilentlyContinue
        Remove-Item $got -ErrorAction SilentlyContinue
        Stop-Job $server -ErrorAction SilentlyContinue; Remove-Job $server -Force -ErrorAction SilentlyContinue
    }
}

# ------------------------------------------------------------ performance
if (Want "perf") {
    Write-Host "perf"
    $s = @"
wait 10
open $w/big.rs
wait 30
click 400 300
perf
key ctrl+end
wait 30
dump $o/perf1.json
type // end of file
key ctrl+home
perf
scroll 0 -4000
scroll 0 -4000
scroll 0 -4000
dump $o/perf2.json
perf
wait 60
dump $o/perf3.json
quit
"@
    if (Run "perf" $s) {
        $j = J "perf1"; $d = Doc $j
        Check "120,000-line file opens" ($d.lines -eq 120001) "lines=$($d.lines)"
        Check "jump to end of a large file stays responsive" ($j.frame_max_ms -lt 60) "max=$([math]::Round($j.frame_max_ms,1)) ms; $($j.slow -join ', ')"
        $j = J "perf2"
        Check "scrolling a large file: average frame under 8 ms" ($j.frame_avg_ms -lt 8) "avg=$([math]::Round($j.frame_avg_ms,2)) ms"
        $j = J "perf3"
        Check "idle frames under 3 ms" ($j.frame_avg_ms -lt 3) "avg=$([math]::Round($j.frame_avg_ms,2)) ms"
        Write-Host ("  info memory with a 120k-line file: {0:N0} MB" -f $j.memory_mb)
    }
}

Write-Host ""
Write-Host "$($script:pass) passed, $($script:fail) failed   (output: $Out)"
if ($script:fail) { Write-Host ("Failed: " + ($script:failed -join "; ")) -ForegroundColor Red; exit 1 }
