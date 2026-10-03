# The companion's per-frame vision path, timed stage by stage, on this PC.
#
#   .\tools\bench.ps1                       # the fixtures, 300 frames, 1366x768 and 1920x1080
#   .\tools\bench.ps1 -Label before         # name the result bench\<date>-before.{txt,json}
#   .\tools\bench.ps1 -Frames 600 -Sizes "1366x768"
#
# Builds vision_bench in release mode and runs it over resources\maplestory.png
# and chaos-zakum-solo-lvl230.mp4 (frames decoded by ffmpeg: the one on the
# PATH, or the one MapleSyrup downloaded for its recordings). The table is
# printed and kept, with everything measured as JSON, under bench\.
param(
    [int]$Frames = 300,
    [string]$Sizes = "1366x768,1920x1080",
    [string]$Label = "",
    [string]$Image = "resources\maplestory.png",
    [string]$Video = "chaos-zakum-solo-lvl230.mp4",
    [double]$Fps = 10
)
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

cargo build --release --bin vision_bench
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$ffmpeg = $null
$onPath = Get-Command ffmpeg -ErrorAction SilentlyContinue
if ($onPath) {
    $ffmpeg = $onPath.Source
} else {
    $downloaded = Join-Path $env:APPDATA "MapleSyrup\ffmpeg\ffmpeg.exe"
    if (Test-Path $downloaded) { $ffmpeg = $downloaded }
}

$stamp = Get-Date -Format "yyyy-MM-dd-HHmm"
$name = if ($Label) { "$stamp-$Label" } else { $stamp }
New-Item -ItemType Directory -Force -Path bench | Out-Null

$arguments = @("--frames", $Frames, "--sizes", $Sizes, "--fps", $Fps, "--json", "bench\$name.json")
if (Test-Path $Image) { $arguments += @("--image", $Image) }
if (Test-Path $Video) {
    if ($ffmpeg) {
        $arguments += @("--video", $Video, "--ffmpeg", $ffmpeg)
    } else {
        Write-Warning "no ffmpeg found; the recording is skipped (install ffmpeg, or record once with MapleSyrup)"
    }
}

& .\target\release\vision_bench.exe @arguments 2>&1 | Tee-Object -FilePath "bench\$name.txt"
Write-Host "`nkept: bench\$name.txt and bench\$name.json"
