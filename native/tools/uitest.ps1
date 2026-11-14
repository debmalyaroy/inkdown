# UI regression suite for the native app.
#
# Drives the real window through INKDOWN_AUTOMATION scripts (synthetic input, no real
# mouse or keyboard) in an isolated profile, then checks the state dumps and files.
#
#   pwsh tools\uitest.ps1                # debug build
#   pwsh tools\uitest.ps1 -Release       # release build
#   pwsh tools\uitest.ps1 -Only editor   # one group
param([switch]$Release, [string]$Only = "", [string]$Out = "")

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root ("target\" + $(if ($Release) { "release" } else { "debug" }) + "\inkdown.exe")
if (-not (Test-Path $exe)) { throw "Build first: $exe not found" }
if (-not $Out) { $Out = Join-Path $env:TEMP "inkdown-uitest" }
Remove-Item -Recurse -Force $Out -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$Out\work\proj\src", "$Out\export" | Out-Null
$o = $Out.Replace('\', '/')
$w = "$o/work"

# ---------------------------------------------------------------- fixtures
Set-Content "$Out\work\a.rs" "fn alpha() {`n    let foo = 1;`n    let bar = foo + foo;`n}`n" -NoNewline
Set-Content "$Out\work\list.txt" "one`ntwo`nthree`nfour`nfive`n" -NoNewline
Set-Content "$Out\work\doc.md" "# Trip plan`n`nPack **light** and bring a *map*.`n`n- [x] Tickets`n- [ ] Hotel`n`n| Day | Place |`n| --- | --- |`n| 1 | Rome |`n`n``````mermaid`nflowchart LR`n  A --> B`n```````n`nMath `$a^2`$ here.`n" -NoNewline
Set-Content "$Out\work\data.json" '{"name":"ink","items":[1,2,{"deep":true}],"n":null}' -NoNewline
Set-Content "$Out\work\bad.json" "{`n  `"a`": 1,`n  oops`n}" -NoNewline
Set-Content "$Out\work\t.csv" "name,qty`napple,3`npear,5`n" -NoNewline
Set-Content "$Out\work\d.svg" "" -NoNewline
Set-Content "$Out\work\hello.bat" "@echo hello from build`r`n" -NoNewline
Set-Content "$Out\work\save.txt" "start`n" -NoNewline
Set-Content "$Out\work\auto.txt" "start`n" -NoNewline
Set-Content "$Out\work\proj\src\main.rs" "fn main() {`n    helper_one();`n}`n" -NoNewline
Set-Content "$Out\work\proj\src\util.rs" "pub fn helper_one() {}`npub fn helper_two() {}`n" -NoNewline
Set-Content "$Out\work\proj\notes.md" "# Notes`n`nhelper_one is used in main.`n" -NoNewline
$big = [System.Text.StringBuilder]::new()
for ($i = 0; $i -lt 120000; $i++) { [void]$big.Append("fn item_$i(x: u32) -> u32 { x + $i } // line $i`n") }
[System.IO.File]::WriteAllText("$Out\work\big.rs", $big.ToString())

$script:pass = 0; $script:fail = 0; $script:failed = @()
function Check([string]$name, [bool]$ok, [string]$detail = "") {
    if ($ok) { $script:pass++; Write-Host "  ok   $name" }
    else { $script:fail++; $script:failed += $name; Write-Host "  FAIL $name  $detail" -ForegroundColor Red }
}
function Run([string]$name, [string]$body, [switch]$Keep, [string[]]$Files = @()) {
    if (-not $Keep) { Remove-Item -Recurse -Force "$Out\profile" -ErrorAction SilentlyContinue }
    Set-Content "$Out\$name.txt" $body
    $env:INKDOWN_DATA_DIR = "$Out\profile"
    $env:INKDOWN_AUTOMATION = "$Out\$name.txt"
    $env:INKDOWN_WINDOW = "1280x840"
    $env:INKDOWN_DIALOG_DIR = "$Out\export"
    $p = if ($Files.Count) { Start-Process -FilePath $exe -ArgumentList $Files -PassThru } else { Start-Process -FilePath $exe -PassThru }
    if (-not $p.WaitForExit(120000)) { $p.Kill(); Check "$name finished" $false "timed out" ; return $false }
    return $true
}
function J([string]$file) { Get-Content "$Out\$file.json" -Raw | ConvertFrom-Json }
function Doc($j) { $j.docs[$j.active] }
function Want([string]$group) { -not $Only -or $Only -eq $group }

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
        Check "screenshot written" ((Test-Path "$Out\start.png") -and (Get-Item "$Out\start.png").Length -gt 20000)
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
        Check "designer drawing saves to disk" (-not (Doc (J "dz2")).dirty -and (Get-Content "$Out\work\d.svg" -Raw).Contains("inkdown-diagram:"))
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
        Check "Ctrl+S writes the file" (-not (Doc (J "f2")).dirty -and (Get-Content "$Out\work\save.txt" -Raw) -eq "start`nsaved line")
        Check "AutoSave writes without Ctrl+S" ((Get-Content "$Out\work\auto.txt" -Raw) -eq "start`nauto line" -and -not (Doc (J "f3")).dirty)
        Check "theme switches" ((J "f4").theme -eq "dracula")
        Check "settings go to the isolated profile" ((Test-Path "$Out\profile\config\settings.json") -and (Get-Content "$Out\profile\config\settings.json" -Raw).Contains('"dracula"'))
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
open $w/hello.bat
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
sleep 40000
dump $o/x1.json
quit
"@
    if (Run "export" $s) {
        $h = "$Out\export\doc.html"
        Check "HTML export" ((Test-Path $h) -and (Get-Content $h -Raw).Contains("<h1 id=`"trip-plan`">Trip plan</h1>"))
        Check "HTML embeds the diagram and math" ((Test-Path $h) -and ([regex]::Matches((Get-Content $h -Raw), "data:image/svg\+xml").Count -ge 2))
        Check "Word export" ((Test-Path "$Out\export\doc.docx") -and (Get-Item "$Out\export\doc.docx").Length -gt 5000)
        Check "RTF export" ((Test-Path "$Out\export\doc.rtf") -and (Get-Content "$Out\export\doc.rtf" -TotalCount 1).StartsWith("{\rtf1"))
        Check "PDF export" ((Test-Path "$Out\export\doc.pdf") -and (Get-Item "$Out\export\doc.pdf").Length -gt 3000)
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
