[CmdletBinding()]
param(
    [string[]]$CargoArguments = @('--locked', '--workspace', '--all-targets'),
    [string[]]$TestArguments = @(),
    [string]$RuntimeDirectory = $env:SHERPA_ONNX_LIB_DIR
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repoRoot = Split-Path -Parent $PSScriptRoot
$tauriRoot = Join-Path $repoRoot 'src-tauri'
if ($env:OS -ne 'Windows_NT') { throw 'Native DLL staging is required only on Windows.' }
if ($PSVersionTable.PSVersion.Major -lt 7) { throw 'Native test wrapper requires PowerShell 7 (pwsh).' }
if (!$RuntimeDirectory -or !(Test-Path -LiteralPath $RuntimeDirectory -PathType Container)) {
    throw 'Run bootstrap-ci-resources.ps1 first, or pass its verified library directory with -RuntimeDirectory.'
}
$runtimeRoot = (Resolve-Path -LiteralPath $RuntimeDirectory).Path
if ($env:SHERPA_ONNX_LIB_DIR -and (Resolve-Path -LiteralPath $env:SHERPA_ONNX_LIB_DIR).Path -ne $runtimeRoot) {
    throw 'RuntimeDirectory must match the Sherpa directory used for linking (SHERPA_ONNX_LIB_DIR).'
}
# Use this exact directory for both the build and executable-adjacent staging.
$env:SHERPA_ONNX_LIB_DIR = $runtimeRoot
foreach ($argument in $CargoArguments) {
    if ($argument -eq '--' -or $argument -eq '--no-run' -or $argument -match '^--(message-format|manifest-path)(=|$)') {
        throw 'CargoArguments contains a wrapper-owned option; use TestArguments for harness options.'
    }
}
$jsonDirectory = Join-Path $repoRoot 'build\checks'
New-Item -ItemType Directory -Force -Path $jsonDirectory | Out-Null
$messagesPath = Join-Path $jsonDirectory ("native-test-artifacts-" + [Guid]::NewGuid().ToString('N') + '.jsonl')

Push-Location $tauriRoot
try {
    $metadataText = & cargo metadata --no-deps --locked --format-version=1
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $targetDirectory = ($metadataText | ConvertFrom-Json).target_directory
    for ($index = 0; $index -lt $CargoArguments.Count; $index++) {
        if ($CargoArguments[$index] -eq '--target-dir') {
            if ($index + 1 -ge $CargoArguments.Count) { throw '--target-dir requires a directory.' }
            $targetDirectory = $CargoArguments[++$index]
        } elseif ($CargoArguments[$index].StartsWith('--target-dir=')) {
            $targetDirectory = $CargoArguments[$index].Substring('--target-dir='.Length)
        }
    }
    $targetDirectory = [IO.Path]::GetFullPath($targetDirectory, $tauriRoot)
    Write-Host 'Compile native tests before staging their linked runtime.'
    & cargo test @CargoArguments --no-run --message-format=json | Set-Content -LiteralPath $messagesPath -Encoding utf8
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    & node (Join-Path $PSScriptRoot 'native-test-runtime.mjs') $messagesPath $runtimeRoot $targetDirectory
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    Write-Host 'Run native tests with the same Cargo selection and the verified runtime beside each executable.'
    if ($TestArguments.Count -gt 0) {
        & cargo test @CargoArguments -- @TestArguments
    } else {
        & cargo test @CargoArguments
    }
    exit $LASTEXITCODE
} finally {
    Pop-Location
    # Only the unique manifest created by this invocation is removed.
    Remove-Item -LiteralPath $messagesPath -Force -ErrorAction SilentlyContinue
}
