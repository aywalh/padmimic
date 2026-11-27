$ErrorActionPreference = 'Stop'
$oldFlags = $env:CARGO_ENCODED_RUSTFLAGS
try {
    $env:CARGO_ENCODED_RUSTFLAGS = @("--remap-path-prefix=$env:USERPROFILE=/build-user", "--remap-path-prefix=$PSScriptRoot=/padmimic") -join [char]31
    cargo build --release --locked --manifest-path (Join-Path $PSScriptRoot 'Cargo.toml')
    if ($LASTEXITCODE -ne 0) { throw "Cargo failed ($LASTEXITCODE)" }
} finally {
    $env:CARGO_ENCODED_RUSTFLAGS = $oldFlags
}
