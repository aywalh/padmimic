# Local release: build, then stop if the exe names this machine.
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot
cargo build --release
if ($LASTEXITCODE) { throw "build failed" }
node ..\release-tools\check-leaks.mjs target\release\padmimic.exe
if ($LASTEXITCODE) { throw "leak check failed" }
