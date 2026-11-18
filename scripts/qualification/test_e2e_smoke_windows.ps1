#Requires -Version 5.1
<#
  Synthetic Windows CLI contract tests for scripts/e2e-proof.ps1.
  No real model, daemon, network access, or CUDA is exercised. This verifies
  only diagnostic honesty and fail-closed behavior of the PowerShell wrapper.
#>
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$scriptPath = Join-Path (Split-Path $PSScriptRoot -Parent) 'e2e-proof.ps1'
$work = Join-Path ([IO.Path]::GetTempPath()) ('ggcore-smoke-' + [guid]::NewGuid().ToString('N'))
$fixtureDir = Join-Path $work 'fixture with spaces'
$fixturePath = Join-Path $fixtureDir 'qwen2.5-0.5b-instruct-q4_k_m.gguf'
$binary = Join-Path $work 'fake-cli.cmd'
$script:cases = 0
$script:failures = 0

function Assert-Case([string]$Name, [string]$Mode, [bool]$ValidAuth,
                     [bool]$ValidFixture, [bool]$ExpectedPass) {
    $script:cases++
    if ($ValidFixture) {
        [IO.File]::WriteAllBytes($fixturePath, [Text.Encoding]::ASCII.GetBytes('GGUFsynthetic-fixture'))
    } elseif (Test-Path $fixturePath) {
        Remove-Item -LiteralPath $fixturePath
    }
    $env:CORE_AUTH_TOKEN = if ($ValidAuth) { 'synthetic-token' } else { '' }
    $env:FAKE_MODE = $Mode
    $output = & pwsh -NoProfile -NonInteractive -File $scriptPath -SkipBuild -ModelsDir $fixtureDir -Binary $binary 2>&1
    $code = $LASTEXITCODE
    $text = ($output | Out-String)
    $passed = $code -eq 0 -and $text.Contains('SMOKE PASS:')
    $correct = if ($ExpectedPass) { $passed } else { $code -ne 0 -and -not $passed }
    if ($correct) {
        Write-Output "PASS $Name"
    } else {
        $script:failures++
        Write-Output "FAIL $Name expected=$ExpectedPass exit=$code output=$text"
    }
    if ($ExpectedPass -and $correct) {
        if (-not $text.Contains('NOT RUN: active cancel') -or
            -not $text.Contains('RELEASE HOLD:')) {
            $script:failures++
            Write-Output "FAIL $Name did not disclose unrun release gates"
        }
    }
}

New-Item -ItemType Directory -Path $fixtureDir -Force | Out-Null
try {
    # .cmd is used so $LASTEXITCODE represents an actual external process.
    $stub = @'
@echo off
if /I "%~1"=="version" (echo {"version":"synthetic"}& exit /b 0)
if /I "%~1"=="status" (
  if /I "%FAKE_MODE%"=="no-daemon" exit /b 7
  echo {"health":"synthetic"}
  exit /b 0
)
if /I "%~1"=="models" (
  if /I "%FAKE_MODE%"=="no-model" (echo {"models":[]}& exit /b 0)
  echo {"models":[{"id":"qwen"}]}
  exit /b 0
)
if /I "%~1"=="infer" (
  if /I "%FAKE_MODE%"=="infer-fail" exit /b 8
  echo %* | findstr /C:"--stream" >nul
  if not errorlevel 1 (
    if /I "%FAKE_MODE%"=="stream-fail" exit /b 9
    echo one two three four five
    exit /b 0
  )
  if /I "%FAKE_MODE%"=="wrong-answer" (echo 5& exit /b 0)
  echo 4
  exit /b 0
)
exit /b 10
'@
    [IO.File]::WriteAllText($binary, $stub, [Text.Encoding]::ASCII)
    Assert-Case 'narrow-smoke-pass' 'ok' $true $true $true
    Assert-Case 'no-auth' 'ok' $false $true $false
    Assert-Case 'no-gguf' 'ok' $true $false $false
    Assert-Case 'no-daemon' 'no-daemon' $true $true $false
    Assert-Case 'no-model-id' 'no-model' $true $true $false
    Assert-Case 'infer-failed' 'infer-fail' $true $true $false
    Assert-Case 'wrong-answer' 'wrong-answer' $true $true $false
    Assert-Case 'stream-failed' 'stream-fail' $true $true $false
} finally {
    Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item Env:CORE_AUTH_TOKEN -ErrorAction SilentlyContinue
    Remove-Item Env:FAKE_MODE -ErrorAction SilentlyContinue
}
Write-Output "RESULT cases=$script:cases failures=$script:failures (synthetic only)"
if ($script:failures -gt 0) { exit 1 }
