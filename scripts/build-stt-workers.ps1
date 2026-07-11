[CmdletBinding()]
param(
    [switch]$SkipCuda,
    [switch]$SkipVulkan
)

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$tauriRoot = Join-Path $repoRoot 'src-tauri'
$workersDir = Join-Path $tauriRoot 'resources\stt-workers'
New-Item -ItemType Directory -Path $workersDir -Force | Out-Null

function Invoke-Native([string]$File, [string[]]$Arguments) {
    & $File @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed ($LASTEXITCODE): $File $($Arguments -join ' ')"
    }
}

function Find-CudaBin {
    $roots = @()
    if ($env:CUDA_PATH) {
        $roots += $env:CUDA_PATH
    }
    $toolkitsRoot = 'C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA'
    if (Test-Path $toolkitsRoot) {
        $roots += Get-ChildItem $toolkitsRoot -Directory |
            Sort-Object Name -Descending |
            ForEach-Object FullName
    }
    foreach ($root in $roots | Select-Object -Unique) {
        $candidate = Join-Path $root 'bin\x64'
        if (Test-Path (Join-Path $candidate 'cublas64_*.dll')) {
            return $candidate
        }
    }
    throw 'CUDA Toolkit with bin\\x64\\cublas64_*.dll was not found. It is required only on the release-build machine.'
}

Push-Location $tauriRoot
try {
    if (!$SkipCuda) {
        Invoke-Native 'cargo' @('build', '-p', 'fono-stt-worker', '--release', '--features', 'cuda', '--target-dir', 'target-worker-cuda')
        Copy-Item 'target-worker-cuda\release\fono-stt-worker.exe' (Join-Path $workersDir 'fono-stt-cuda-worker.exe') -Force

        # whisper.cpp CUDA imports cuBLAS dynamically. These redistributable DLLs
        # let an NVIDIA user run the packaged worker without installing the Toolkit.
        $cudaBin = Find-CudaBin
        foreach ($pattern in @('cublas64_*.dll', 'cublasLt64_*.dll', 'cudart64_*.dll')) {
            $runtime = Get-ChildItem $cudaBin -Filter $pattern | Sort-Object Name -Descending | Select-Object -First 1
            if (!$runtime) {
                throw "CUDA runtime $pattern was not found in $cudaBin"
            }
            Copy-Item $runtime.FullName $workersDir -Force
        }
    }

    if (!$SkipVulkan) {
        Invoke-Native 'cmake' @('-S', 'crates\fono-stt-vulkan-worker', '-B', 'target-vulkan-worker', '-A', 'x64', '-DCMAKE_BUILD_TYPE=Release')
        Invoke-Native 'cmake' @('--build', 'target-vulkan-worker', '--config', 'Release', '--target', 'fono-stt-vulkan-worker', '--parallel', '8')
        Copy-Item 'target-vulkan-worker\Release\fono-stt-vulkan-worker.exe' (Join-Path $workersDir 'fono-stt-vulkan-worker.exe') -Force
    }
} finally {
    Pop-Location
}

Write-Host "STT workers prepared in $workersDir"
