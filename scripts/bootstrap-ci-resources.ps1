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

function Assert-DownloadSpec($Dependency) {
    $uri = [Uri]$Dependency.url
    if ($uri.Scheme -ne 'https' -or $Dependency.sha256 -notmatch '^[0-9a-f]{64}$') {
        throw 'Dependency manifest requires HTTPS and a pinned SHA-256.'
    }
    if ($Dependency.downloadTimeoutSeconds -isnot [long] -and $Dependency.downloadTimeoutSeconds -isnot [int]) {
        throw 'Dependency manifest requires an integer download timeout.'
    }
    if ($Dependency.downloadTimeoutSeconds -lt 30 -or $Dependency.downloadTimeoutSeconds -gt 1800) {
        throw 'Dependency download timeout must be between 30 and 1800 seconds.'
    }
}

function Get-VerifiedDownload($Dependency) {
    Assert-DownloadSpec $Dependency
    $uri = [Uri]$Dependency.url
    $fileName = [IO.Path]::GetFileName($uri.AbsolutePath)
    $destination = Join-Path $workRoot $fileName
    if (Test-Path -LiteralPath $destination -PathType Leaf) {
        Write-Host "Verifying cached dependency: $fileName"
        Assert-Checksum $destination $Dependency.sha256
        return $destination
    }
    $partial = "$destination.part"
    $attemptLimit = 3
    for ($attempt = 1; $attempt -le $attemptLimit; $attempt++) {
        $watch = [Diagnostics.Stopwatch]::StartNew()
        try {
            Remove-Item -LiteralPath $partial -Force -ErrorAction SilentlyContinue
            Write-Host "Downloading $fileName (attempt $attempt/$attemptLimit, total limit $($Dependency.downloadTimeoutSeconds)s)."
            # PowerShell TimeoutSec bounds connection setup only; curl max-time also bounds the response body.
            & curl.exe --disable --fail --silent --show-error --location --proto '=https' --proto-redir '=https' --connect-timeout 30 --max-time $Dependency.downloadTimeoutSeconds --speed-time 30 --speed-limit 1024 --output $partial --url $uri.AbsoluteUri
            if ($LASTEXITCODE -ne 0) { throw "Dependency download failed (curl exit $LASTEXITCODE)." }
            $downloadBytes = (Get-Item -LiteralPath $partial).Length
            Write-Host "Downloaded ${fileName}: $downloadBytes bytes in $([Math]::Round($watch.Elapsed.TotalSeconds, 1))s; verifying SHA-256."
            Assert-Checksum $partial $Dependency.sha256
            Move-Item -LiteralPath $partial -Destination $destination -Force
            Write-Host "Verified dependency: $fileName ($([Math]::Round($watch.Elapsed.TotalSeconds, 1))s)."
            return $destination
        } catch {
            Remove-Item -LiteralPath $partial -Force -ErrorAction SilentlyContinue
            Write-Warning "$fileName attempt $attempt/$attemptLimit failed after $([Math]::Round($watch.Elapsed.TotalSeconds, 1))s: $($_.Exception.Message)"
            if ($attempt -eq $attemptLimit) { throw }
            Start-Sleep -Seconds 2
        } finally {
            $watch.Stop()
        }
    }
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
        Assert-DownloadSpec $dependency
    }
    Write-Host 'Pinned native manifest, Sherpa lock version and VAD checksum verified.'
    return
}

if (![Environment]::Is64BitOperatingSystem -or $env:OS -ne 'Windows_NT') { throw 'Bootstrap requires Windows x64.' }
if ($InstallGpuSdks -and $env:GITHUB_ACTIONS -ne 'true') {
    throw 'Automatic SDK installation is restricted to GitHub Actions build machines. Local SDK installation is manual.'
}
$tarExecutable = Join-Path $env:SystemRoot 'System32\tar.exe'
if (!(Test-Path -LiteralPath $tarExecutable -PathType Leaf)) { throw 'Windows System32 tar.exe is required.' }
# Git GNU tar interprets the colon in an absolute Windows archive path as a remote host.
Write-Host "Using Windows archive tool: $tarExecutable"
& $tarExecutable --version
if ($LASTEXITCODE -ne 0) { throw 'Cannot verify Windows tar version.' }
New-Item -ItemType Directory -Path $workRoot -Force | Out-Null
$archive = Get-VerifiedDownload $manifest.sherpa
Write-Host 'Inspecting verified Sherpa archive paths.'
$entries = & $tarExecutable -tf $archive
if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect Sherpa archive.' }
foreach ($entry in $entries) {
    if ($entry -match '(^[\\/]|^[A-Za-z]:|(^|[\\/])\.\.([\\/]|$))' -or ($entry.TrimEnd('/') -ne $manifest.sherpa.directory -and !$entry.StartsWith("$($manifest.sherpa.directory)/"))) {
        throw 'Sherpa archive has an unexpected extraction path.'
    }
}
Write-Host 'Extracting verified Sherpa runtime.'
& $tarExecutable -xf $archive -C $workRoot
if ($LASTEXITCODE -ne 0) { throw 'Cannot extract Sherpa archive.' }
$libDir = Join-Path $workRoot "$($manifest.sherpa.directory)\lib"
$destination = Join-Path $repoRoot 'src-tauri\resources\sherpa-onnx'
New-Item -ItemType Directory -Path $destination -Force | Out-Null
foreach ($name in @($requiredDlls) + @('sherpa-onnx-c-api.lib', 'onnxruntime.lib')) {
    if (!(Test-Path -LiteralPath (Join-Path $libDir $name) -PathType Leaf)) { throw "Missing Sherpa library: $name" }
}
Write-Host 'Staging verified Sherpa DLLs.'
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
