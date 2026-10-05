[CmdletBinding()]
param([switch]$SkipChecks)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($PSVersionTable.PSVersion.Major -lt 7) { throw 'Use PowerShell 7 (pwsh) to build FonoSetup.' }
if ($env:OS -ne 'Windows_NT') { throw 'FonoSetup distribution is built on Windows x64.' }
if ($env:PROCESSOR_ARCHITECTURE -ne 'AMD64') { throw 'Use a Windows x64 build host.' }

$repoRoot = Split-Path -Parent $PSScriptRoot
$crateRoot = Join-Path $repoRoot 'tools\fono-setup'
$version = (Get-Content -LiteralPath (Join-Path $repoRoot 'package.json') -Raw | ConvertFrom-Json).version
$channel = Get-Content -LiteralPath (Join-Path $repoRoot 'src-tauri\update-channel.json') -Raw | ConvertFrom-Json
$outputDirectory = Join-Path $repoRoot 'build\online-installer'
New-Item -ItemType Directory -Force -Path $outputDirectory | Out-Null

Push-Location $crateRoot
try {
    # Building from the crate root applies its static CRT flags without changing
    # Fono's own GPU/Whisper build settings. This is an independent Cargo workspace.
    if (!$SkipChecks) {
        & cargo fmt --all --check
        if ($LASTEXITCODE -ne 0) { throw 'FonoSetup formatting failed.' }
        & cargo test --locked --all-targets
        if ($LASTEXITCODE -ne 0) { throw 'FonoSetup tests failed.' }
        & cargo clippy --locked --all-targets -- -D warnings
        if ($LASTEXITCODE -ne 0) { throw 'FonoSetup clippy failed.' }
    }
    & cargo build --release --locked --target x86_64-pc-windows-msvc --bin fono-setup
    if ($LASTEXITCODE -ne 0) { throw 'FonoSetup release build failed.' }
    $sourcePath = Join-Path $crateRoot 'target\x86_64-pc-windows-msvc\release\fono-setup.exe'
    $source = Get-Item -LiteralPath $sourcePath
    if ($source.Length -le 0 -or $source.Length -gt 16MB) { throw 'FonoSetup must be a non-empty executable under 16 MiB.' }
    if ($source.VersionInfo.ProductVersion -ne $version -or $source.VersionInfo.OriginalFilename -ne 'FonoSetup.exe') {
        throw 'FonoSetup embedded product version or filename differs from source metadata.'
    }
    $destination = Join-Path $outputDirectory 'FonoSetup.exe'
    Copy-Item -LiteralPath $sourcePath -Destination $destination -Force
    $digest = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant()
    Set-Content -LiteralPath (Join-Path $outputDirectory 'FonoSetup.exe.sha256') -Value "$digest  FonoSetup.exe" -Encoding utf8NoBOM
    $revision = (& git -C $repoRoot rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0) { throw 'Cannot read source revision.' }
    $dirty = [bool]((& git -C $repoRoot status --porcelain --untracked-files=normal -- tools/fono-setup scripts/build-online-installer.ps1 package.json src-tauri/update-channel.json) -join '')
    [ordered]@{
        bootstrap_version = $version
        source_revision = $revision
        source_dirty = $dirty
        endpoint = $channel.endpoint
        bytes = $source.Length
        sha256 = $digest
        built_at = [datetime]::UtcNow.ToString('o')
        installed = $false
    } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $outputDirectory 'build-info.json') -Encoding utf8NoBOM
    Write-Host "FonoSetup ${version}: $([math]::Round($source.Length / 1MB, 2)) MiB"
    Write-Host "Prepared: $destination"
    Write-Host 'The online installer has not been launched, installed or published.'
} finally {
    Pop-Location
}
