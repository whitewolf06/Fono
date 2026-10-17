[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($PSVersionTable.PSVersion.Major -lt 7) { throw 'CPU development requires PowerShell 7 (pwsh).' }
if ($env:OS -ne 'Windows_NT' -or ![Environment]::Is64BitOperatingSystem) { throw 'CPU development requires Windows x64.' }
$repoRoot = Split-Path -Parent $PSScriptRoot
$tauriCli = Join-Path $repoRoot 'node_modules/@tauri-apps/cli/tauri.js'
$cpuConfig = Join-Path $repoRoot 'src-tauri/tauri.cpu.conf.json'
if (!(Test-Path -LiteralPath $tauriCli -PathType Leaf)) { throw 'Tauri CLI is missing. Run npm ci before npm run dev:cpu.' }
if (!(Test-Path -LiteralPath $cpuConfig -PathType Leaf)) { throw 'CPU Tauri configuration is missing.' }
$node = Get-Command node.exe -CommandType Application -ErrorAction Stop | Select-Object -First 1
$cacheMaintenance = Join-Path $PSScriptRoot 'clean-build-cache.mjs'
function Invoke-AvailableCacheMaintenance {
    if (Test-Path -LiteralPath $cacheMaintenance -PathType Leaf) {
        & $node.Source $cacheMaintenance --apply --if-needed
        if ($LASTEXITCODE -ne 0) { throw 'Build cache maintenance failed.' }
    }
}
if ($env:FONO_TEST_DATA_DIR -and ![IO.Path]::IsPathFullyQualified($env:FONO_TEST_DATA_DIR)) {
    throw 'FONO_TEST_DATA_DIR must be absolute.'
}

$environmentNames = @(
    'PATH', 'SHERPA_ONNX_LIB_DIR', 'WHISPER_DONT_GENERATE_BINDINGS',
    'GGML_CUDA', 'GGML_VULKAN', 'WHISPER_CUBLAS', 'WHISPER_CUDA', 'WHISPER_VULKAN',
    'FONO_PREPARING_RELEASE_RESOURCES', 'TAURI_CONFIG', 'FONO_TEST_DATA_DIR', 'FONO_API_PORT'
)
$previousEnvironment = @{}
foreach ($name in $environmentNames) { $previousEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
$exitCode = 1
Push-Location $repoRoot
try {
    Invoke-AvailableCacheMaintenance
    # Keep the verified Sherpa link directory and PATH in this very process.
    # The bootstrap never installs GPU SDKs without an explicit separate switch.
    & (Join-Path $PSScriptRoot 'bootstrap-ci-resources.ps1')

    # Upstream bundled bindings describe a Linux ABI; Windows requires bindgen
    # with LLVM/libclang even when inference uses only the embedded CPU backend.
    Remove-Item -LiteralPath Env:WHISPER_DONT_GENERATE_BINDINGS -ErrorAction SilentlyContinue
    foreach ($name in @('GGML_CUDA', 'GGML_VULKAN')) {
        [Environment]::SetEnvironmentVariable($name, 'OFF', 'Process')
    }
    foreach ($name in @('WHISPER_CUBLAS', 'WHISPER_CUDA', 'WHISPER_VULKAN')) {
        Remove-Item -LiteralPath "Env:$name" -ErrorAction SilentlyContinue
    }
    Remove-Item -LiteralPath Env:FONO_PREPARING_RELEASE_RESOURCES -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath Env:TAURI_CONFIG -ErrorAction SilentlyContinue
    if (!$env:FONO_TEST_DATA_DIR) { $env:FONO_TEST_DATA_DIR = Join-Path $repoRoot 'build/dev-cpu-profile' }
    if (!$env:FONO_API_PORT) { $env:FONO_API_PORT = '17833' }

    # Tauri CLI's first -- forwards Cargo runner arguments; application
    # arguments would require a second --. No app arguments are supplied.
    & $node.Source $tauriCli dev --config $cpuConfig --features cpu-only -- --no-default-features --locked
    $exitCode = $LASTEXITCODE
} finally {
    try {
        Invoke-AvailableCacheMaintenance
    } finally {
        Pop-Location
        foreach ($name in $environmentNames) {
            if ($null -eq $previousEnvironment[$name]) {
                Remove-Item -LiteralPath "Env:$name" -ErrorAction SilentlyContinue
            } else {
                [Environment]::SetEnvironmentVariable($name, $previousEnvironment[$name], 'Process')
            }
        }
    }
}
exit $exitCode
