param(
    [ValidateRange(1, 100000)][int]$Cycles = 1000,
    [ValidateRange(1, 10000)][int]$Faults = 100,
    [ValidateRange(0, 7200)][int]$IdleSeconds = 0,
    [string]$Artifacts = 'build/lifecycle-soak/tests.jsonl',
    [string]$Report = 'build/lifecycle-soak/report.json',
    [switch]$DownloadModel
)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $projectRoot
$targetDirectory = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $projectRoot 'src-tauri/target-codex-review' }
$workerPath = Join-Path $targetDirectory 'release/fono-stt-worker.exe'
$messages = Get-Content -LiteralPath $Artifacts | ForEach-Object { $_ | ConvertFrom-Json }
$finished = @($messages | Where-Object { $_.reason -eq 'build-finished' -and $_.success })
$drivers = @($messages | Where-Object { $_.reason -eq 'compiler-artifact' -and $_.target.name -eq 'fono_lib' -and $_.profile.test -and $_.executable })
if ($finished.Count -ne 1 -or $drivers.Count -ne 1) { throw 'Expected one successfully emitted Fono library test executable' }
$driverPath = [IO.Path]::GetFullPath($drivers[0].executable)
$resolvedTarget = [IO.Path]::GetFullPath($targetDirectory).TrimEnd('\') + '\'
if (-not $driverPath.StartsWith($resolvedTarget, [StringComparison]::OrdinalIgnoreCase)) { throw 'Cargo test driver is outside selected target directory' }
$modelPath = Join-Path $projectRoot 'build/stt-quality/ggml-tiny.bin'
$modelSha = 'be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21'
if (-not (Test-Path -LiteralPath $modelPath)) {
    if (-not $DownloadModel) { throw 'Pinned public tiny model missing; pass -DownloadModel explicitly' }
    New-Item -ItemType Directory -Force (Split-Path -Parent $modelPath) | Out-Null
    # Same immutable public model revision as scripts/stt-quality.mjs.
    $modelSpec = Get-Content -LiteralPath 'scripts/stt-quality.mjs' -Raw
    $revision = [regex]::Match($modelSpec, 'revision: "([0-9a-f]+)"').Groups[1].Value
    if ($revision.Length -ne 40) { throw 'Pinned public model revision missing' }
    $downloadPath = "$modelPath.download-$([Guid]::NewGuid().ToString('N'))"
    try {
        Invoke-WebRequest -Uri "https://huggingface.co/ggerganov/whisper.cpp/resolve/$revision/ggml-tiny.bin" -OutFile $downloadPath
        if ((Get-FileHash -LiteralPath $downloadPath -Algorithm SHA256).Hash.ToLowerInvariant() -ne $modelSha) { throw 'Public model SHA256 mismatch' }
        Move-Item -LiteralPath $downloadPath -Destination $modelPath
    } finally {
        if (Test-Path -LiteralPath $downloadPath) { Remove-Item -LiteralPath $downloadPath }
    }
}
$driverSha = (Get-FileHash -LiteralPath $driverPath -Algorithm SHA256).Hash.ToLowerInvariant()
$workerSha = (Get-FileHash -LiteralPath $workerPath -Algorithm SHA256).Hash.ToLowerInvariant()
$activeEvery = [Math]::Min(100, $Cycles)
node scripts/lifecycle-soak.mjs --runner $driverPath --runner-sha256 $driverSha --worker $workerPath --worker-sha256 $workerSha --worker-profile release-cpu-no-default-features --model $modelPath --cycles $Cycles --faults $Faults --active-cancel-every $activeEvery --idle-seconds $IdleSeconds --report $Report
exit $LASTEXITCODE
