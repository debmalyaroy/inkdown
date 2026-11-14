# Inkdown build entry point for Windows (PowerShell 7). Same targets as the Makefile.
#
#   pwsh build.ps1               release build
#   pwsh build.ps1 test          unit tests
#   pwsh build.ps1 package       dist\Inkdown-Setup.exe and dist\inkdown.exe
#   pwsh build.ps1 help
param(
    [ValidateSet('release', 'build', 'run', 'test', 'check', 'uitest', 'package', 'windows', 'msix', 'android', 'clean', 'version', 'help')]
    [string]$Task = 'release'
)
$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
$native = Join-Path $root 'native'
$dist = Join-Path $root 'dist'
$exe = Join-Path $native 'target\release\inkdown.exe'
$version = (Select-String -Path (Join-Path $native 'Cargo.toml') -Pattern '^version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value

function Cargo([string[]]$CargoArgs) {
    Push-Location $native
    try {
        & cargo @CargoArgs
        if ($LASTEXITCODE -ne 0) { throw "cargo $($CargoArgs -join ' ') failed ($LASTEXITCODE)" }
    } finally { Pop-Location }
}

switch ($Task) {
    'help' {
        Write-Host "Inkdown $version"
        Write-Host "  release   optimised build ($exe)"
        Write-Host "  build     debug build"
        Write-Host "  run       run the debug build"
        Write-Host "  test      unit tests"
        Write-Host "  check     type-check only"
        Write-Host "  uitest    release build, then the UI regression suite"
        Write-Host "  package   dist\Inkdown-Setup.exe (installer) and dist\inkdown.exe (portable)"
        Write-Host "  msix      Microsoft Store package (needs the Windows SDK)"
        Write-Host "  android   dist\android\inkdown.apk (needs Docker)"
        Write-Host "  clean     remove build output"
    }
    'version' { Write-Output $version }
    'build' { Cargo @('build', '--locked') }
    'release' { Cargo @('build', '--release', '--locked') }
    'run' { Cargo @('run', '--locked') }
    'test' { Cargo @('test', '--locked') }
    'check' { Cargo @('check', '--locked') }
    'uitest' {
        Cargo @('build', '--release', '--locked')
        & pwsh (Join-Path $native 'tools\uitest.ps1') -Release
        if ($LASTEXITCODE -ne 0) { throw 'UI tests failed' }
    }
    { $_ -in 'package', 'windows' } {
        Cargo @('build', '--release', '--locked')
        New-Item -ItemType Directory -Force $dist | Out-Null
        # The installer is the application itself: an exe whose name contains "setup" installs.
        Copy-Item $exe (Join-Path $dist 'inkdown.exe') -Force
        Copy-Item $exe (Join-Path $dist 'Inkdown-Setup.exe') -Force
        Get-ChildItem $dist -Filter '*.exe' | ForEach-Object { '{0}  {1:N1} MB' -f $_.Name, ($_.Length / 1MB) }
    }
    'msix' {
        Cargo @('build', '--release', '--locked')
        & pwsh (Join-Path $root 'packaging\windows\make-msix.ps1') -Exe $exe -Version $version
        if ($LASTEXITCODE -ne 0) { throw 'MSIX packaging failed' }
    }
    'android' {
        Push-Location $root
        try {
            docker build -f packaging/android/Dockerfile -t inkdown-android .
            if ($LASTEXITCODE -ne 0) { throw 'Android build failed' }
            New-Item -ItemType Directory -Force (Join-Path $dist 'android') | Out-Null
            docker run --rm -v "$(Join-Path $dist 'android'):/out" inkdown-android
        } finally { Pop-Location }
    }
    'clean' {
        Cargo @('clean')
        Remove-Item -Recurse -Force $dist -ErrorAction SilentlyContinue
    }
}
