[CmdletBinding()]
param(
    [switch]$InstallGpuSdks,
    [switch]$ValidateOnly
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repoRoot = Split-Path -Parent $PSScriptRoot
$manifest = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'ci-native-dependencies.json') -Raw | ConvertFrom-Json
$workRoot = Join-Path $repoRoot 'build\ci-dependencies'
$requiredDlls = @('onnxruntime.dll', 'onnxruntime_providers_shared.dll', 'sherpa-onnx-c-api.dll', 'sherpa-onnx-cxx-api.dll')

function Assert-Checksum([string]$Path, [string]$Expected) {
    if (!(Test-Path -LiteralPath $Path -PathType Leaf)) { throw "Missing resource: $Path" }
    if ((Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Expected) {
        throw "SHA-256 mismatch: $Path"
    }
}

function Get-VerifiedDownload($Dependency) {
    $uri = [Uri]$Dependency.url
    if ($uri.Scheme -ne 'https' -or $Dependency.sha256 -notmatch '^[0-9a-f]{64}$') {
        throw 'Dependency manifest requires HTTPS and a pinned SHA-256.'
    }
    $destination = Join-Path $workRoot ([IO.Path]::GetFileName($uri.AbsolutePath))
    if (!(Test-Path -LiteralPath $destination -PathType Leaf)) {
        $partial = "$destination.part"
        Invoke-WebRequest -Uri $uri -OutFile $partial -TimeoutSec 1800
        Assert-Checksum $partial $Dependency.sha256
        Move-Item -LiteralPath $partial -Destination $destination -Force
    }
    Assert-Checksum $destination $Dependency.sha256
    return $destination
}

function Set-BuildEnvironment([string]$Name, [string]$Value) {
    [Environment]::SetEnvironmentVariable($Name, $Value, 'Process')
    if ($env:GITHUB_ENV) { Add-Content -LiteralPath $env:GITHUB_ENV -Value "$Name=$Value" -Encoding utf8 }
}

function Add-BuildPath([string]$Directory) {
    $env:PATH = "$Directory;$env:PATH"
    if ($env:GITHUB_PATH) { Add-Content -LiteralPath $env:GITHUB_PATH -Value $Directory -Encoding utf8 }
}

function Invoke-HiddenInstaller([string]$File, [string[]]$Arguments) {
    $process = Start-Process -FilePath $File -ArgumentList $Arguments -WindowStyle Hidden -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "SDK installation failed with exit code $($process.ExitCode)." }
}

$cargoLock = Get-Content -LiteralPath (Join-Path $repoRoot 'src-tauri\Cargo.lock') -Raw
$expectedVersion = [regex]::Escape($manifest.sherpa.version)
if ($cargoLock -notmatch "name = `"sherpa-onnx-sys`"\r?\nversion = `"$expectedVersion`"") {
    throw 'Sherpa lock version changed: update CI dependency URL and SHA-256 together.'
}
Assert-Checksum (Join-Path $repoRoot $manifest.vad.path) $manifest.vad.sha256
if ($ValidateOnly) {
    foreach ($dependency in @($manifest.sherpa, $manifest.cuda, $manifest.vulkan)) {
        if ([Uri]$dependency.url -and ([Uri]$dependency.url).Scheme -ne 'https') { throw 'Insecure dependency URL.' }
        if ($dependency.sha256 -notmatch '^[0-9a-f]{64}$') { throw 'Invalid dependency SHA-256.' }
    }
    Write-Host 'Pinned native manifest, Sherpa lock version and VAD checksum verified.'
    return
}

if (![Environment]::Is64BitOperatingSystem -or $env:OS -ne 'Windows_NT') { throw 'Bootstrap requires Windows x64.' }
if ($InstallGpuSdks -and $env:GITHUB_ACTIONS -ne 'true') {
    throw 'Automatic SDK installation is restricted to GitHub Actions build machines. Local SDK installation is manual.'
}
New-Item -ItemType Directory -Path $workRoot -Force | Out-Null
$archive = Get-VerifiedDownload $manifest.sherpa
$entries = & tar -tf $archive
if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect Sherpa archive.' }
foreach ($entry in $entries) {
    if ($entry -match '(^[\\/]|^[A-Za-z]:|(^|[\\/])\.\.([\\/]|$))' -or ($entry.TrimEnd('/') -ne $manifest.sherpa.directory -and !$entry.StartsWith("$($manifest.sherpa.directory)/"))) {
        throw 'Sherpa archive has an unexpected extraction path.'
    }
}
& tar -xf $archive -C $workRoot
if ($LASTEXITCODE -ne 0) { throw 'Cannot extract Sherpa archive.' }
$libDir = Join-Path $workRoot "$($manifest.sherpa.directory)\lib"
$destination = Join-Path $repoRoot 'src-tauri\resources\sherpa-onnx'
New-Item -ItemType Directory -Path $destination -Force | Out-Null
foreach ($name in @($requiredDlls) + @('sherpa-onnx-c-api.lib', 'onnxruntime.lib')) {
    if (!(Test-Path -LiteralPath (Join-Path $libDir $name) -PathType Leaf)) { throw "Missing Sherpa library: $name" }
}
foreach ($name in $requiredDlls) {
    Copy-Item -LiteralPath (Join-Path $libDir $name) -Destination (Join-Path $destination $name) -Force
}
Set-BuildEnvironment 'SHERPA_ONNX_LIB_DIR' $libDir
Add-BuildPath $libDir
Write-Host "Verified Sherpa $($manifest.sherpa.version) runtime prepared from its official archive."

if ($InstallGpuSdks) {
    $cudaInstaller = Get-VerifiedDownload $manifest.cuda
    Invoke-HiddenInstaller $cudaInstaller @('-s', 'nvcc_13.3', 'cudart_13.3', 'crt_13.3', 'cublas_13.3', 'cublas_dev_13.3', 'nvvm_13.3', 'thrust_13.3', 'visual_studio_integration_13.3')
    $cudaRoot = $manifest.cuda.directory
    if (!(Test-Path -LiteralPath (Join-Path $cudaRoot 'bin\nvcc.exe'))) { throw 'CUDA compiler was not installed.' }
    & (Join-Path $cudaRoot 'bin\nvcc.exe') --version
    if ($LASTEXITCODE -ne 0) { throw 'CUDA compiler check failed.' }
    Set-BuildEnvironment 'CUDA_PATH' $cudaRoot
    Add-BuildPath (Join-Path $cudaRoot 'bin')
    Add-BuildPath (Join-Path $cudaRoot 'bin\x64')

    $vulkanRoot = Join-Path $workRoot "vulkan-$($manifest.vulkan.version)"
    $vulkanInstaller = Get-VerifiedDownload $manifest.vulkan
    Invoke-HiddenInstaller $vulkanInstaller @('--root', "`"$vulkanRoot`"", '--accept-licenses', '--default-answer', '--confirm-command', 'install', 'copy_only=1')
    foreach ($name in @('Bin\glslc.exe', 'Include\vulkan\vulkan.h', 'Lib\vulkan-1.lib')) {
        if (!(Test-Path -LiteralPath (Join-Path $vulkanRoot $name))) { throw "Vulkan SDK lacks $name" }
    }
    Set-BuildEnvironment 'VULKAN_SDK' $vulkanRoot
    Set-BuildEnvironment 'VK_SDK_PATH' $vulkanRoot
    Add-BuildPath (Join-Path $vulkanRoot 'Bin')
    Write-Host 'Pinned CUDA and Vulkan SDKs prepared. No GPU inference has been tested.'
}
