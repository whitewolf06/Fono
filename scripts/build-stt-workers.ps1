[CmdletBinding()]
param(
    [switch]$SkipCuda,
    [switch]$SkipVulkan,
    [ValidateNotNullOrEmpty()][string]$CudaArchitectures = '75;80;86;89;90;100;120'
)

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$tauriRoot = Join-Path $repoRoot 'src-tauri'
$workersDir = Join-Path $tauriRoot 'resources\stt-workers'
New-Item -ItemType Directory -Path $workersDir -Force | Out-Null

# Replace only the backend built in this invocation, after its build succeeds.
# SkipCuda/SkipVulkan preserve the other already prepared backend.

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
        # Distribution binaries must not inherit host-only GPU autodetection.
        # A different matrix is an explicit local-build choice.
        $cudaTarget = Join-Path $tauriRoot 'target-worker-cuda'
        $cudaMatrixStamp = Join-Path $cudaTarget 'cuda-architectures.txt'
        $previousMatrix = if (Test-Path -LiteralPath $cudaMatrixStamp -PathType Leaf) {
            (Get-Content -LiteralPath $cudaMatrixStamp -Raw).Trim()
        } else { '' }
        if ((Test-Path -LiteralPath $cudaTarget) -and $previousMatrix -ne $CudaArchitectures) {
            # whisper-rs-sys forwards CMAKE_* flags but does not register them
            # as Cargo rebuild inputs. Invalidate only this dependency when
            # changing the matrix or migrating a cache without a stamp.
            $resolvedTarget = (Resolve-Path -LiteralPath $cudaTarget).Path
            if ($resolvedTarget -ne [IO.Path]::GetFullPath($cudaTarget) -or
                ((Get-Item -LiteralPath $cudaTarget).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
                throw 'Refusing to refresh a linked CUDA worker cache.'
            }
            $linkedCache = Get-ChildItem -LiteralPath $cudaTarget -Recurse -Force |
                Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint } |
                Select-Object -First 1
            if ($linkedCache) { throw 'Refusing to refresh CUDA worker cache containing links.' }
            $trackedCache = @(& git -C $repoRoot ls-files -- 'src-tauri/target-worker-cuda')
            if ($LASTEXITCODE -ne 0 -or $trackedCache.Count -gt 0) { throw 'Cannot verify that the CUDA worker cache is untracked.' }
            Invoke-Native 'cargo' @('clean', '--release', '-p', 'whisper-rs-sys', '--target-dir', $cudaTarget)
        }
        $previousCudaArchitectures = $env:CMAKE_CUDA_ARCHITECTURES
        $env:CMAKE_CUDA_ARCHITECTURES = $CudaArchitectures
        try {
            Write-Host "CUDA architecture matrix: $CudaArchitectures"
            Invoke-Native 'cargo' @('build', '-p', 'fono-stt-worker', '--bin', 'fono-stt-worker', '--release', '--features', 'cuda', '--target-dir', 'target-worker-cuda')
            Set-Content -LiteralPath $cudaMatrixStamp -Value $CudaArchitectures -Encoding ascii
        } finally {
            if ($null -eq $previousCudaArchitectures) {
                Remove-Item Env:CMAKE_CUDA_ARCHITECTURES -ErrorAction SilentlyContinue
            } else {
                $env:CMAKE_CUDA_ARCHITECTURES = $previousCudaArchitectures
            }
        }
        Get-ChildItem -LiteralPath $workersDir -File |
            Where-Object { $_.Name -eq 'fono-stt-cuda-worker.exe' -or $_.Name -match '^(cublas64_|cublasLt64_|cudart64_).*\.dll$' } |
            ForEach-Object { Remove-Item -LiteralPath $_.FullName -Force }
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
        Invoke-Native 'cmake' @('-S', '..\crates\fono-stt-vulkan-worker', '-B', 'target-vulkan-worker', '-A', 'x64', '-DCMAKE_BUILD_TYPE=Release')
        Invoke-Native 'cmake' @('--build', 'target-vulkan-worker', '--config', 'Release', '--target', 'fono-stt-vulkan-worker', '--parallel', '8')
        Copy-Item 'target-vulkan-worker\Release\fono-stt-vulkan-worker.exe' (Join-Path $workersDir 'fono-stt-vulkan-worker.exe') -Force
    }
} finally {
    Pop-Location
}

Write-Host "STT workers prepared in $workersDir"
$backends = @()
if (!$SkipCuda) { $backends += 'cuda' }
if (!$SkipVulkan) { $backends += 'vulkan' }
if ($backends.Count -gt 0) {
    & (Join-Path $PSScriptRoot 'verify-stt-workers.ps1') -WorkersDirectory $workersDir -Backends $backends
}
