# Runs the end-to-end UI checks of the Linux build in a Docker container.
#
#   pwsh packaging/linux/uitest.ps1                       # uses dist/linux/inkdown
#   pwsh packaging/linux/uitest.ps1 -Binary path/to/inkdown
#   pwsh packaging/linux/uitest.ps1 -Only editor          # one group
#
# Needs Docker. The Linux binary comes from `make linux`, from a release archive, or from
# the "inkdown-linux" artifact of a CI run (gh run download <run> -n inkdown-linux).
param([string]$Binary = "", [string]$Only = "", [string]$Image = "inkdown-linux-uitest")

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not $Binary) { $Binary = Join-Path $repo "dist/linux/inkdown" }
if (-not (Test-Path $Binary)) { throw "No Linux binary at $Binary. Build it with 'make linux' or download the CI artifact, then pass -Binary." }
$app = Split-Path -Parent (Resolve-Path $Binary).Path
if ((Split-Path -Leaf $Binary) -ne "inkdown") { throw "The binary must be named 'inkdown' (it is mounted as /app/inkdown)." }
$out = Join-Path $repo "dist/linux-uitest"
New-Item -ItemType Directory -Force $out | Out-Null

docker build -q -t $Image (Join-Path $PSScriptRoot "uitest") | Out-Null
if ($LASTEXITCODE -ne 0) { throw "docker build failed" }
$more = @()
if ($Only) { $more = @("-Only", $Only) }
docker run --rm --shm-size=512m -v "${repo}:/src:ro" -v "${app}:/app:ro" -v "${out}:/out" $Image @more
exit $LASTEXITCODE
