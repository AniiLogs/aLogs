# Builds a preview installer of the Rust aLogs (no release, no betas.json, no update signing):
#   powershell -NoProfile -ExecutionPolicy Bypass -File tools\package-preview.ps1 [-Rc 9000] [-Beta 0]
# 1. release builds of the meter, Sanctum Helper and the OCR helper (from reference/aLogs/app/ocr-helper);
# 2. the app's resources folders laid out as the installer expects (all git-ignored: Sanctum Helper's Assets hold the
#    game's map art, never committed): resources/meter (alogs-meter.exe, the shipped data, ocr-helper.exe),
#    resources/sanctumhelper (SanctumHelper.exe and its Assets), resources/vendor (HidHide);
# 3. build.json with a high RC number, so the app's updater doesn't replace this build with the published 0.1.x of the
#    same version (a newer version still does);
# 4. `tauri build` (@tauri-apps/cli 2.12.4, as the reference pins) without update artifacts: nothing is signed.
# The installer lands in target/release/bundle/nsis/.
param([int]$Rc = 9000, [int]$Beta = 0)
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$ref = Join-Path $root "reference"
$app = Join-Path $root "crates\alogs-app"
$res = Join-Path $app "resources"

Push-Location $root
try {
    cargo build --release -p alogs-meter -p alogs-sanctum
    if ($LASTEXITCODE) { throw "cargo build failed" }
    cargo build --release --manifest-path (Join-Path $ref "aLogs\app\ocr-helper\Cargo.toml")
    if ($LASTEXITCODE) { throw "ocr-helper build failed" }

    foreach ($d in "meter", "sanctumhelper", "vendor") {
        $p = Join-Path $res $d
        if (Test-Path $p) { Get-ChildItem $p -Force | Where-Object { $_.Name -ne ".gitkeep" } | Remove-Item -Recurse -Force }
        else { New-Item -ItemType Directory $p | Out-Null }
    }
    $bossbar = Join-Path $ref "aLogs\bossbar"
    $meter = Join-Path $res "meter"
    Copy-Item (Join-Path $root "target\release\alogs-meter.exe") $meter
    foreach ($f in "plugins.json", "boss_catalog.csv", "boss_rush_grades.csv", "boss_rush_cycles.json", "xn_templates.json", "boss_break.json") {
        Copy-Item (Join-Path $bossbar $f) $meter
    }
    Copy-Item (Join-Path $ref "aLogs\app\ocr-helper\target\release\ocr-helper.exe") $meter
    Copy-Item (Join-Path $ref "aLogs\app\ocr-helper\model\LICENSE-model.txt") $meter
    $sanctum = Join-Path $res "sanctumhelper"
    Copy-Item (Join-Path $root "target\release\SanctumHelper.exe") $sanctum
    Copy-Item (Join-Path $ref "SanctumHelper\app\SanctumHelper\Assets") (Join-Path $sanctum "Assets") -Recurse
    Copy-Item (Join-Path $bossbar "vendor\*") (Join-Path $res "vendor") -Recurse

    $version = (Get-Content (Join-Path $app "tauri.conf.json") -Raw | ConvertFrom-Json).version
    Set-Content (Join-Path $app "build.json") ("{""version"":""$version"",""rc"":$Rc,""beta"":$Beta}") -Encoding ascii -NoNewline

    Push-Location $app
    try {
        npx --yes @tauri-apps/cli@2.12.4 build --config '{\"bundle\":{\"createUpdaterArtifacts\":false}}'
        if ($LASTEXITCODE) { throw "tauri build failed" }
    } finally { Pop-Location }
    Get-ChildItem (Join-Path $root "target\release\bundle\nsis") -Filter *.exe | ForEach-Object { "installer: $($_.FullName) ({0:N1} MB)" -f ($_.Length / 1MB) }
} finally { Pop-Location }
