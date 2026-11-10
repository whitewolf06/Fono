[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$fonoDevRoot = Split-Path -Parent $PSScriptRoot
& (Join-Path $PSScriptRoot 'import-desktop-dev-environment.ps1') -RepositoryRoot $fonoDevRoot
Push-Location -LiteralPath $fonoDevRoot
try {
    & (Join-Path $fonoDevRoot 'node_modules\.bin\tauri.cmd') dev
    exit $LASTEXITCODE
} finally {
    Pop-Location
}
