[CmdletBinding()]
param([Parameter(Mandatory = $true)][string]$RepositoryRoot)

$ErrorActionPreference = 'Stop'
$fonoDevConfigPath = Join-Path $RepositoryRoot 'build\desktop-dev.json'
$fonoWakePython = $env:FONO_WAKE_DEV_PYTHON
if (!$fonoWakePython -and (Test-Path -LiteralPath $fonoDevConfigPath -PathType Leaf)) {
    $fonoDevConfig = Get-Content -Raw -LiteralPath $fonoDevConfigPath | ConvertFrom-Json
    $fonoWakePython = $fonoDevConfig.wakeDevPython
}
if ($fonoWakePython) {
    if (![IO.Path]::IsPathFullyQualified($fonoWakePython) -or !(Test-Path -LiteralPath $fonoWakePython -PathType Leaf)) {
        throw 'FONO_WAKE_DEV_PYTHON must point to an existing absolute Python executable.'
    }
    # Capability only. The detector and microphone still require explicit enable.
    $env:FONO_WAKE_DEV_PYTHON = [IO.Path]::GetFullPath($fonoWakePython)
}
