[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$tauriRoot = Join-Path $repoRoot 'src-tauri'
$sherpaDir = Join-Path $tauriRoot 'resources\sherpa-onnx'
$targetDir = Join-Path $tauriRoot 'target-release-resources'
$requiredSherpaDlls = @(
    'onnxruntime.dll',
    'onnxruntime_providers_shared.dll',
    'sherpa-onnx-c-api.dll',
    'sherpa-onnx-cxx-api.dll'
)

function Invoke-Native([string]$File, [string[]]$Arguments) {
    & $File @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed ($LASTEXITCODE): $File $($Arguments -join ' ')"
    }
}

& (Join-Path $PSScriptRoot 'build-stt-workers.ps1')

New-Item -ItemType Directory -Path $sherpaDir -Force | Out-Null
Get-ChildItem -LiteralPath $sherpaDir -File |
    Where-Object { $_.Extension -eq '.dll' } |
    ForEach-Object { Remove-Item -LiteralPath $_.FullName -Force }

Push-Location $tauriRoot
try {
    # This preliminary build produces the Sherpa runtime beside its release
    # executable. The environment flag only skips the final bundle-manifest
    # check while those DLLs do not exist yet; the subsequent Tauri build runs
    # without it and validates the complete manifest.
    $previousPreparing = $env:FONO_PREPARING_RELEASE_RESOURCES
    $env:FONO_PREPARING_RELEASE_RESOURCES = '1'
    try {
        Invoke-Native 'cargo' @('build', '--release', '--target-dir', $targetDir)
    } finally {
        if ($null -eq $previousPreparing) {
            Remove-Item Env:FONO_PREPARING_RELEASE_RESOURCES -ErrorAction SilentlyContinue
        } else {
            $env:FONO_PREPARING_RELEASE_RESOURCES = $previousPreparing
        }
    }

    foreach ($name in $requiredSherpaDlls) {
        $source = Join-Path $targetDir "release\$name"
        if (!(Test-Path -LiteralPath $source -PathType Leaf)) {
            throw "Required Sherpa runtime DLL was not produced: $source"
        }
        Copy-Item -LiteralPath $source -Destination (Join-Path $sherpaDir $name) -Force
    }
} finally {
    Pop-Location
}

$stagedNames = @(Get-ChildItem -LiteralPath $sherpaDir -File -Filter '*.dll' | ForEach-Object Name)
$unexpected = @($stagedNames | Where-Object { $_ -notin $requiredSherpaDlls })
$missing = @($requiredSherpaDlls | Where-Object { $_ -notin $stagedNames })
if ($missing.Count -gt 0 -or $unexpected.Count -gt 0) {
    throw "Invalid Sherpa runtime manifest. Missing: $($missing -join ', '); unexpected: $($unexpected -join ', ')"
}

Write-Host "Release resources prepared: STT workers and Sherpa runtime manifest."

& (Join-Path $PSScriptRoot 'clean-legacy-release-artifacts.ps1')
