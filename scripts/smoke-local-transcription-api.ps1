[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$AudioPath,

    [ValidateSet('tiny', 'base', 'small', 'medium', 'large', 'large_turbo')]
    [string]$Model = 'large_turbo',

    [string]$Language = 'auto',

    [ValidateRange(1, 600)]
    [int]$TimeoutSeconds = 300,

    [switch]$VerifyCancellation,

    [string]$BaseUrl = 'http://127.0.0.1:17832'
)

$ErrorActionPreference = 'Stop'

function Get-ApiToken {
    $path = Join-Path $env:APPDATA 'Fono\api-token.txt'
    if (!(Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Fono API token was not found at $path. Start Fono once and copy the token in Service -> API."
    }

    $token = (Get-Content -LiteralPath $path -Raw).Trim()
    if ([string]::IsNullOrWhiteSpace($token)) {
        throw 'Fono API token file is empty.'
    }
    return $token
}

function Invoke-ApiGet([string]$Path, [hashtable]$Headers) {
    return Invoke-RestMethod "$BaseUrl$Path" -Headers $Headers
}

function Invoke-ApiPost([string]$Path, [hashtable]$Headers) {
    return Invoke-RestMethod "$BaseUrl$Path" -Method Post -Headers $Headers
}

function Submit-Transcription([string]$Path) {
    $response = & curl.exe --silent --show-error --fail `
        "$BaseUrl/v1/transcriptions" `
        -H "Authorization: Bearer $token" `
        -F "audio=@$Path" `
        -F "model=$Model" `
        -F "language=$Language"
    if ($LASTEXITCODE -ne 0) {
        throw "Audio upload failed with curl exit code $LASTEXITCODE."
    }
    return $response | ConvertFrom-Json
}

function Wait-ForTerminalJob([string]$Id) {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $job = Invoke-ApiGet "/v1/transcription-jobs/$Id" $headers
        if ($job.state -in @('completed', 'failed', 'cancelled')) {
            return $job
        }
        Start-Sleep -Seconds 1
    } while ((Get-Date) -lt $deadline)

    throw "Job $Id did not reach a terminal state within $TimeoutSeconds seconds."
}

$token = Get-ApiToken
$headers = @{ Authorization = "Bearer $token" }

$health = Invoke-ApiGet '/v1/health' $headers
if ($health.state -ne 'ready') {
    throw "Unexpected health state: $($health.state)"
}

$docs = Invoke-WebRequest "$BaseUrl/docs" -UseBasicParsing
if ($docs.StatusCode -ne 200 -or $docs.Content -notmatch '<title>Fono Local API</title>') {
    throw 'Browser API documentation was not served as expected.'
}

$specification = Invoke-WebRequest "$BaseUrl/openapi.json" -Headers $headers -UseBasicParsing
if ($specification.StatusCode -ne 200 -or $specification.Headers['Content-Type'] -notmatch 'application/json') {
    throw 'Authenticated OpenAPI endpoint was not served as JSON.'
}

$job = Submit-Transcription (Resolve-Path -LiteralPath $AudioPath)
$terminalJob = Wait-ForTerminalJob $job.id
if ($terminalJob.state -ne 'completed' -or [string]::IsNullOrWhiteSpace($terminalJob.result.text)) {
    throw "Transcription job $($job.id) ended as $($terminalJob.state)."
}

$summary = [ordered]@{
    health_state = $health.state
    protocol_version = $health.protocol_version
    browser_docs = 'ok'
    openapi = 'ok'
    transcription_job = $terminalJob.id
    transcription_state = $terminalJob.state
    model = $terminalJob.result.model
    backend = $terminalJob.result.backend
    text_length = $terminalJob.result.text.Length
}

if ($VerifyCancellation) {
    $cancelled = Submit-Transcription (Resolve-Path -LiteralPath $AudioPath)
    $cancelResult = Invoke-ApiPost "/v1/transcription-jobs/$($cancelled.id)/cancel" $headers
    $cancelledJob = Wait-ForTerminalJob $cancelled.id
    if ($cancelResult.state -ne 'cancelled' -or $cancelledJob.state -ne 'cancelled') {
        throw "Cancellation job $($cancelled.id) ended as $($cancelledJob.state). Use a longer audio file if it completed before cancellation."
    }
    $summary.cancellation_job = $cancelledJob.id
    $summary.cancellation_state = $cancelledJob.state
}

[PSCustomObject]$summary | Format-List
