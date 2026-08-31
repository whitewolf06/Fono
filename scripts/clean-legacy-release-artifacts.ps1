[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$releaseDir = Join-Path $repoRoot 'src-tauri\target\release'
$legacyArtifacts = @(
    'whisperclone_lib.dll',
    'whisperclone_lib.dll.exp',
    'whisperclone_lib.dll.lib',
    'whisperclone_lib.lib',
    'whisperclone_lib.pdb',
    'whisperclone_lib.d',
    'libwhisperclone_lib.rlib',
    'libwhisperclone_lib.rmeta',
    'libwhisperclone_lib.d',
    'whisperclone.exe',
    'whisperclone.pdb',
    'whisperclone.d'
)

foreach ($name in $legacyArtifacts) {
    $path = Join-Path $releaseDir $name
    if (Test-Path -LiteralPath $path -PathType Leaf) {
        Remove-Item -LiteralPath $path -Force
        Write-Host "Removed legacy release artifact: $name"
    }
}
