[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Container })]
    [string]$ModelDir,

    [string]$TargetDir = (Join-Path $env:TEMP "fono-wake-model-smoke")
)

$ErrorActionPreference = "Stop"

$repositoryRoot = Split-Path -Parent $PSScriptRoot
$tauriRoot = Join-Path $repositoryRoot "src-tauri"
$runtimeDir = Join-Path $tauriRoot "resources\sherpa-onnx"
$testDepsDir = Join-Path $TargetDir "debug\deps"
$runtimeDlls = @(
    "onnxruntime_providers_shared.dll",
    "onnxruntime.dll",
    "sherpa-onnx-c-api.dll",
    "sherpa-onnx-cxx-api.dll"
)
$previousModelDir = $env:FONO_KWS_MODEL_DIR
$bundledTestWav = Join-Path $ModelDir "test_wavs\0.wav"

if (-not (Test-Path -LiteralPath $bundledTestWav -PathType Leaf)) {
    throw "The GigaSpeech model is incomplete: expected bundled test WAV at $bundledTestWav. Download the full model archive before running this smoke."
}

Push-Location $tauriRoot
try {
    # The test executable must load the bundled ONNX Runtime, not an older
    # onnxruntime.dll that Windows can otherwise resolve from System32.
    cargo test -p fono-wake --features sherpa-wake --target-dir $TargetDir --no-run
    if ($LASTEXITCODE -ne 0) {
        throw "Could not build the Sherpa wake-word smoke tests."
    }

    $testExecutable = Get-ChildItem -LiteralPath $testDepsDir -Filter "fono_wake-*.exe" |
        Sort-Object LastWriteTimeUtc -Descending |
        Select-Object -First 1
    if ($null -eq $testExecutable) {
        throw "The fono-wake test executable was not created in $testDepsDir."
    }

    foreach ($runtimeDll in $runtimeDlls) {
        Copy-Item -LiteralPath (Join-Path $runtimeDir $runtimeDll) -Destination $testExecutable.DirectoryName -Force
    }

    $env:FONO_KWS_MODEL_DIR = (Resolve-Path -LiteralPath $ModelDir)
    cargo test -p fono-wake --features sherpa-wake --target-dir $TargetDir downloaded_model -- --ignored
    if ($LASTEXITCODE -ne 0) {
        throw "Sherpa wake-word model smoke failed."
    }
}
finally {
    if ($null -eq $previousModelDir) {
        Remove-Item Env:FONO_KWS_MODEL_DIR -ErrorAction SilentlyContinue
    }
    else {
        $env:FONO_KWS_MODEL_DIR = $previousModelDir
    }
    Pop-Location
}
