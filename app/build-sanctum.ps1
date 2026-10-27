# Builds Sanctum Helper (../SanctumHelper, the friend's repo) into src-tauri/resources/sanctumhelper for the installer.
# Same publish as its own build.ps1, without its installer: aLogs runs it with --hosted. It uses the PC's shared .NET
# Desktop Runtime (9 or newer) instead of carrying its own copy (~150 MB); the installer offers that runtime.
$ErrorActionPreference = "Stop"
$source = Join-Path $PSScriptRoot "..\SanctumHelper\app\SanctumHelper\SanctumHelper.csproj"
$out = Join-Path $PSScriptRoot "src-tauri\resources\sanctumhelper"
if (Test-Path $out) { Remove-Item $out -Recurse -Force }
dotnet publish $source -c Release -o $out -p:SelfContained=false -p:RollForward=Major
if ($LASTEXITCODE) { throw "Sanctum Helper publish failed" }
# OpenCV's video I/O plugin is never used (it only reads screen pixels and PNGs).
Remove-Item "$out\opencv_videoio_ffmpeg*.dll" -ErrorAction SilentlyContinue
$size = (Get-ChildItem $out -Recurse | Measure-Object Length -Sum).Sum / 1MB
"Sanctum Helper packaged: $out ({0:N1} MB)" -f $size
