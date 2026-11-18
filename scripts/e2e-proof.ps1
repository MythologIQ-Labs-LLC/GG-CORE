#Requires -Version 5.1
<#
.SYNOPSIS
  GG-CORE offline developer smoke, not release or CoreForge acceptance.
.DESCRIPTION
  Tests a pre-existing authenticated local daemon with a locally staged Qwen
  GGUF fixture. Does not download, sign, start a daemon, or claim cancellation,
  security/memory lifetime, streamed chunk cadence, or package qualification.
  The release gate remains scripts/rc-acceptance.sh plus P0 issue #219.
#>
[CmdletBinding()]
param(
    [string]$ModelsDir = "models",
    [string]$ModelId = "qwen",
    [string]$Binary = "",
    [switch]$SkipBuild,
    [switch]$SkipDownload
)
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Fail([string]$Reason) { throw "SMOKE FAIL: $Reason" }
Write-Output "INFO: GG-CORE developer smoke only. NOT a package, cancel or release PASS."
if ([string]::IsNullOrWhiteSpace($env:CORE_AUTH_TOKEN)) {
    Fail "CORE_AUTH_TOKEN is required for authenticated local IPC"
}
if ([string]::IsNullOrWhiteSpace($ModelId)) { Fail "ModelId cannot be empty" }
$modelFile = Join-Path $ModelsDir "qwen2.5-0.5b-instruct-q4_k_m.gguf"
if (-not (Test-Path -LiteralPath $modelFile -PathType Leaf)) {
    Fail "local GGUF fixture missing: $modelFile (no implicit download)"
}
$stream = [System.IO.File]::OpenRead((Resolve-Path -LiteralPath $modelFile).Path)
try {
    $magicBytes = New-Object byte[] 4
    $n = $stream.Read($magicBytes, 0, 4)
} finally {
    $stream.Dispose()
}
if ($n -ne 4 -or [System.Text.Encoding]::ASCII.GetString($magicBytes) -ne "GGUF") {
    Fail "fixture is not GGUF (possibly a Git LFS pointer)"
}
$hash = (Get-FileHash -LiteralPath $modelFile -Algorithm SHA256).Hash.ToLowerInvariant()
Write-Output "INFO: Local fixture SHA-256 (not proof of daemon custody): $hash"

$root = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..")).Path
if (-not $SkipBuild) {
    Push-Location (Join-Path $root "core-runtime")
    try {
        & cargo build --locked --offline --release --features gguf --bin gg-core-cli
        if ($LASTEXITCODE -ne 0) { Fail "locked offline GGUF CLI build failed" }
    } finally {
        Pop-Location
    }
}
if (-not $Binary) {
    $Binary = Join-Path $root "core-runtime\target\release\gg-core-cli.exe"
}
if (-not (Test-Path -LiteralPath $Binary -PathType Leaf)) {
    Fail "CLI executable missing: $Binary"
}
Write-Output "INFO: CLI version (not proof of loaded daemon identity):"
& $Binary version --json
if ($LASTEXITCODE -ne 0) { Fail "CLI version query failed" }

$status = & $Binary status --json
if ($LASTEXITCODE -ne 0 -or -not $status) {
    Fail "authenticated daemon is not responding; start the local daemon with the pinned model"
}
$modelsJson = & $Binary models list --json
if ($LASTEXITCODE -ne 0) { Fail "models list failed over authenticated IPC" }
try {
    $modelResponse = ($modelsJson -join [Environment]::NewLine) | ConvertFrom-Json -ErrorAction Stop
} catch {
    Fail "invalid daemon model-list JSON"
}
if ($null -eq $modelResponse -or $null -eq $modelResponse.models) {
    Fail "daemon model-list response omitted models array"
}
# The wire contract uses ModelInfo.name and ModelInfo.state. A substring
# match can falsely accept another model whose name merely contains the ID.
$readyModels = @($modelResponse.models | Where-Object {
    $_.name -ceq $ModelId -and $_.state -ceq "ready"
})
if ($readyModels.Count -eq 0) {
    Fail "model '$ModelId' not listed with exact name and ready state"
}
Write-Output "INFO: Daemon responds and lists exact ready model; artifact custody remains unverified."

$answer = & $Binary infer --model $ModelId --prompt "What is 2 + 2? Answer with the digit." --max-tokens 32
if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace(($answer -join [Environment]::NewLine))) {
    Fail "non-streaming inference did not produce a successful response"
}
if (($answer -join " ") -notmatch '(?<!\d)4(?!\d)') {
    Fail "non-streaming answer did not contain expected digit 4"
}
$streamed = & $Binary infer --model $ModelId --prompt "Count from one to five in words." --max-tokens 64 --stream
if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace(($streamed -join [Environment]::NewLine))) {
    Fail "stream-mode CLI invocation did not produce a response"
}
Write-Output "INFO: Stream-mode command returned output, not a proven multi-frame cadence."
Write-Output "NOT RUN: active cancel, client drop, native stop, resource lifetime, GPU, or package acceptance."
Write-Output "NOT VERIFIED: metrics delta and loaded model artifact identity."
Write-Output "SMOKE PASS: authenticated local CLI inference and stream-mode output only."
Write-Output "RELEASE HOLD: scripts/rc-acceptance.sh and P0 #219 native tests remain required."
