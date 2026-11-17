# Builds a preview installer of aLogs from the Rust workspace without the update-signing key (a developer's PC, or a
# build for testers that published builds of the same version mustn't replace):
#   powershell -NoProfile -ExecutionPolicy Bypass -File tools\package-preview.ps1 [-Rc 9000] [-Beta 0]
# The same steps as `npm run build:rc` in app/ (patch notes, the game-data check, the OCR helper, meter and Sanctum
# Helper, then app/resources.mjs laying out what the installer carries), then build.json with a high RC number, so the
# app's updater doesn't replace this build with a published one of the same version (a newer version still does), and
# `tauri build` without update artifacts: nothing is signed, and no RC counter or betas.json is touched.
# Needs `npm install` in app/ and Sanctum Helper's checkout as SanctumHelper/ (see app/resources.mjs).
# The installer lands in target/release/bundle/nsis/ as aLogs_<version>_preview-RC<n>_x64-setup.exe, a name the
# publish scripts (share.mjs, release.mjs) never pick up.
param([int]$Rc = 9000, [int]$Beta = 0)
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$appDir = Join-Path $root "app"
$crate = Join-Path $root "crates\alogs-app"
$cli = Join-Path $appDir "node_modules\@tauri-apps\cli\tauri.js"
if (-not (Test-Path $cli)) { throw "No Tauri CLI in app\node_modules: run npm install in app\ first." }

Push-Location $appDir
try {
    foreach ($step in "notes", "meter:build", "sanctum:build", "resources") {
        npm run $step
        if ($LASTEXITCODE) { throw "npm run $step failed" }
    }
} finally { Pop-Location }

$version = (Get-Content (Join-Path $crate "tauri.conf.json") -Raw | ConvertFrom-Json).version
Set-Content (Join-Path $crate "build.json") ("{""version"":""$version"",""rc"":$Rc,""beta"":$Beta}") -Encoding ascii -NoNewline

Push-Location $crate
try {
    node $cli build --config '{\"bundle\":{\"createUpdaterArtifacts\":false}}'
    if ($LASTEXITCODE) { throw "tauri build failed" }
} finally { Pop-Location }
$nsis = Join-Path $root "target\release\bundle\nsis"
$built = Join-Path $nsis "aLogs_${version}_x64-setup.exe"
$preview = Join-Path $nsis "aLogs_${version}_preview-RC${Rc}_x64-setup.exe"
Move-Item $built $preview -Force
"installer: $preview ({0:N1} MB)" -f ((Get-Item $preview).Length / 1MB)
