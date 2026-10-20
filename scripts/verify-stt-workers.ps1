[CmdletBinding()]
param(
    [string]$WorkersDirectory,
    [ValidateSet('cuda', 'vulkan')][string[]]$Backends = @('cuda', 'vulkan')
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
if (!$WorkersDirectory) { $WorkersDirectory = Join-Path $repoRoot 'src-tauri\resources\stt-workers' }
$WorkersDirectory = [IO.Path]::GetFullPath($WorkersDirectory)
$protocolSource = Get-Content -LiteralPath (Join-Path $repoRoot 'src-tauri\crates\fono-stt-protocol\src\lib.rs') -Raw
if ($protocolSource -notmatch 'PROTOCOL_VERSION:\s*u16\s*=\s*(\d+)') { throw 'Cannot read STT protocol version.' }
$protocolVersion = [int]$Matches[1]

foreach ($backend in $Backends) {
    $executable = Join-Path $WorkersDirectory "fono-stt-$backend-worker.exe"
    if (!(Test-Path -LiteralPath $executable -PathType Leaf)) { throw "Missing $backend worker: $executable" }
    # The shared contract verifies the handshake, every required capability,
    # transport limits and lifecycle. Avoid a second PowerShell ReadLineAsync
    # transport with different timeout/framing behavior.
    & node (Join-Path $PSScriptRoot 'worker-contract.mjs') $executable $backend
    if ($LASTEXITCODE -ne 0) { throw "$backend worker failed the shared process contract" }
    Write-Host "$backend worker: protocol $protocolVersion, capabilities and process contract verified"
}
