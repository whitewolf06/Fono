[CmdletBinding()]
param([string]$WorkersDirectory)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
if (!$WorkersDirectory) { $WorkersDirectory = Join-Path $repoRoot 'src-tauri\resources\stt-workers' }
$WorkersDirectory = [IO.Path]::GetFullPath($WorkersDirectory)
$protocolSource = Get-Content -LiteralPath (Join-Path $repoRoot 'src-tauri\crates\fono-stt-protocol\src\lib.rs') -Raw
if ($protocolSource -notmatch 'PROTOCOL_VERSION:\s*u16\s*=\s*(\d+)') { throw 'Cannot read STT protocol version.' }
$protocolVersion = [int]$Matches[1]

foreach ($backend in @('cuda', 'vulkan')) {
    $executable = Join-Path $WorkersDirectory "fono-stt-$backend-worker.exe"
    if (!(Test-Path -LiteralPath $executable -PathType Leaf)) { throw "Missing $backend worker: $executable" }
    $start = New-Object Diagnostics.ProcessStartInfo
    $start.FileName = $executable
    $start.WorkingDirectory = $WorkersDirectory
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardInput = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $process = New-Object Diagnostics.Process
    $process.StartInfo = $start
    $started = $false
    try {
        if (!$process.Start()) { throw "Cannot start $backend worker" }
        $started = $true
        $stderr = $process.StandardError.ReadToEndAsync()
        $request = @{ type = 'hello'; protocol_version = $protocolVersion; request_id = 'release-verification' } | ConvertTo-Json -Compress
        $process.StandardInput.WriteLine($request)
        $process.StandardInput.Flush()
        $responseTask = $process.StandardOutput.ReadLineAsync()
        if (!$responseTask.Wait(5000)) { throw "$backend worker handshake timed out" }
        $response = $responseTask.Result | ConvertFrom-Json
        if ($response.type -ne 'ready' -or $response.protocol_version -ne $protocolVersion -or $response.request_id -ne 'release-verification' -or $response.backend -ne $backend) {
            throw "$backend worker is incompatible with STT protocol $protocolVersion; rebuild workers"
        }
        foreach ($capability in @('supports_window', 'supports_cancel', 'supports_token_timestamps', 'supports_health', 'supports_shutdown')) {
            if (!$response.capabilities.$capability) { throw "$backend worker lacks $capability" }
        }
        if ($response.capabilities.protocol_version -ne $protocolVersion -or $response.capabilities.maximum_request_bytes -lt 16777216 -or $response.capabilities.maximum_response_bytes -lt 1048576) {
            throw "$backend worker publishes incompatible transport limits"
        }
        Write-Host "$backend worker: protocol $protocolVersion, window timestamps and cancellation verified"
    } finally {
        if ($started) {
            try { $process.StandardInput.Close() } catch {}
            if (!$process.HasExited -and !$process.WaitForExit(1000)) { $process.Kill(); $process.WaitForExit() }
        }
        $process.Dispose()
    }
}
