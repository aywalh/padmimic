# Local build. Official release builds come from GitHub Actions (.github/workflows/release.yml).
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot
cargo build --release
if ($LASTEXITCODE) { throw "build failed" }
# optional local leak check (not part of this repo)
if (Test-Path ..\release-tools\check-leaks.mjs) {
    node ..\release-tools\check-leaks.mjs target\release\padmimic.exe
    if ($LASTEXITCODE) { throw "leak check failed" }
}
