#Requires -Version 5.1
<#
.SYNOPSIS
  Negative/positive regression tests for the app-local MSVC runtime stage/verify scripts.
.DESCRIPTION
  Windows only. Stages the real licensed Visual Studio x64 redistributables into a
  temporary directory, then mutates copies and asserts the verifier fails closed with
  the expected reason. No network, no install, no GG-CORE binary is executed: a stage
  without gg-core-cli.exe proves every runtime check passed when the verifier stops at
  the missing-CLI check. Exit 0 only if every case behaves as expected.
#>
[CmdletBinding()]
param([string]$RedistRoot)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$stageScript = Join-Path $PSScriptRoot 'stage-windows-msvc-runtime.ps1'
$verifyScript = Join-Path $PSScriptRoot 'verify-windows-msvc-runtime.ps1'
$work = Join-Path ([System.IO.Path]::GetTempPath()) ("ggcore-msvc-test-" + [guid]::NewGuid().ToString('N'))
$script:failures = 0
$script:passes = 0

function Invoke-Verify([string]$Dir) {
  try { & $verifyScript -StageDirectory $Dir | Out-Null; return 'PASS' } catch { return $_.Exception.Message }
}
function Assert-Case([string]$Name, [string]$Actual, [string]$ExpectPattern) {
  if ($Actual -match $ExpectPattern) { $script:passes++; Write-Output "ok   $Name" }
  else { $script:failures++; Write-Output "FAIL $Name :: expected /$ExpectPattern/ got: $Actual" }
}
function New-Case([string]$Name) {
  $dir = Join-Path $work $Name
  Copy-Item -LiteralPath (Join-Path $work 'base') -Destination $dir -Recurse
  return $dir
}
function Edit-Manifest([string]$Dir, [scriptblock]$Mutate) {
  $path = Join-Path $Dir 'MSVC_REDISTRIBUTABLES.json'
  $json = Get-Content -LiteralPath $path -Raw | ConvertFrom-Json
  & $Mutate $json
  [System.IO.File]::WriteAllText($path, ($json | ConvertTo-Json -Depth 5), (New-Object System.Text.UTF8Encoding $false))
}

New-Item -ItemType Directory -Path (Join-Path $work 'base') -Force | Out-Null
try {
  $stageArgs = @{ StageDirectory = (Join-Path $work 'base') }
  if ($RedistRoot) { $stageArgs.RedistRoot = $RedistRoot }
  $staged = & $stageScript @stageArgs
  Assert-Case 'stage genuine redistributables' ([string]$staged) '^PASS: staged 4 '
  try { & $stageScript @stageArgs | Out-Null; $again = 'PASS' } catch { $again = $_.Exception.Message }
  Assert-Case 'stage refuses to overwrite' $again 'already exists'
  Assert-Case 'genuine runtime passes every check' (Invoke-Verify (Join-Path $work 'base')) 'no gg-core-cli\.exe'

  $d = New-Case 'missing'; Remove-Item -LiteralPath (Join-Path $d 'vcomp140.dll')
  Assert-Case 'missing DLL' (Invoke-Verify $d) 'Missing required packaged vcomp140\.dll'

  $d = New-Case 'corrupt'; $p = Join-Path $d 'msvcp140.dll'
  $bytes = [System.IO.File]::ReadAllBytes($p); $bytes[$bytes.Length / 2] = $bytes[$bytes.Length / 2] -bxor 0xFF
  [System.IO.File]::WriteAllBytes($p, $bytes)
  Assert-Case 'corrupted DLL' (Invoke-Verify $d) 'invalid Authenticode signature: msvcp140\.dll'

  $d = New-Case 'unsigned'
  Copy-Item -LiteralPath (Join-Path $d 'MSVC_REDISTRIBUTABLES.json') -Destination (Join-Path $d 'vcruntime140_1.dll') -Force
  Assert-Case 'unsigned DLL' (Invoke-Verify $d) 'Unsigned or invalid Authenticode signature: vcruntime140_1\.dll'

  $d = New-Case 'unexpected'; Edit-Manifest $d { param($m) $m.windows_runtime[0].file = 'evil.dll' }
  Assert-Case 'unexpected manifest entry' (Invoke-Verify $d) 'Unexpected or duplicate'

  $d = New-Case 'extra-entry'; Edit-Manifest $d { param($m) $m.windows_runtime += $m.windows_runtime[0] }
  Assert-Case 'extra manifest entry' (Invoke-Verify $d) 'Unexpected number'

  $d = New-Case 'wrong-version'; Edit-Manifest $d { param($m) $m.windows_runtime[1].version = '14.0.24215.1' }
  Assert-Case 'incorrect manifest version' (Invoke-Verify $d) 'Runtime version mismatch'

  $d = New-Case 'hash'; Edit-Manifest $d { param($m) $m.windows_runtime[2].sha256 = ('0' * 64) }
  Assert-Case 'tampered manifest hash' (Invoke-Verify $d) 'SHA-256 mismatch'

  $d = New-Case 'older'; Edit-Manifest $d { param($m) $m.source_version = '99.0.0' }
  Assert-Case 'runtime older than declared set' (Invoke-Verify $d) 'older than its declared'

  $d = New-Case 'arch'; Edit-Manifest $d { param($m) $m.architecture = 'x86' }
  Assert-Case 'wrong architecture manifest' (Invoke-Verify $d) 'schema or architecture'

  $d = New-Case 'no-manifest'; Remove-Item -LiteralPath (Join-Path $d 'MSVC_REDISTRIBUTABLES.json')
  Assert-Case 'missing provenance manifest' (Invoke-Verify $d) 'manifest missing'

  # A validly signed, non-Microsoft binary must be refused by signer identity.
  $thirdParty = @("$env:ProgramFiles\nodejs\node.exe", "$env:ProgramFiles\Git\cmd\git.exe",
                  "$env:ProgramFiles\GitHub CLI\gh.exe", "$env:ProgramFiles\PowerShell\7\pwsh.exe") |
    Where-Object { Test-Path -LiteralPath $_ } |
    Where-Object { $s = Get-AuthenticodeSignature -LiteralPath $_; $s.Status -eq 'Valid' -and $s.SignerCertificate.Subject -notmatch 'O=Microsoft Corporation' } |
    Select-Object -First 1
  if ($thirdParty) {
    $d = New-Case 'third-party'; Copy-Item -LiteralPath $thirdParty -Destination (Join-Path $d 'vcomp140.dll') -Force
    Assert-Case 'validly signed non-Microsoft DLL' (Invoke-Verify $d) 'Non-Microsoft runtime signer: vcomp140\.dll'
  } else { Write-Output 'skip validly signed non-Microsoft DLL (no third-party signed binary found on this host)' }

  # A genuinely Microsoft-signed DLL from a different runtime build must still be refused.
  $other = Join-Path $env:SystemRoot 'System32\msvcp140.dll'
  $baseVersion = (Get-Item -LiteralPath (Join-Path $work 'base\msvcp140.dll')).VersionInfo.FileVersion
  if ((Test-Path -LiteralPath $other) -and (Get-Item -LiteralPath $other).VersionInfo.FileVersion -ne $baseVersion) {
    $d = New-Case 'mixed'; Copy-Item -LiteralPath $other -Destination (Join-Path $d 'msvcp140.dll') -Force
    $h = (Get-FileHash -LiteralPath $other -Algorithm SHA256).Hash.ToLowerInvariant()
    $v = (Get-Item -LiteralPath $other).VersionInfo.FileVersion
    Edit-Manifest $d { param($m) foreach ($e in $m.windows_runtime) { if ($e.file -eq 'msvcp140.dll') { $e.sha256 = $h; $e.version = $v } } }
    Assert-Case 'mixed Microsoft runtime versions' (Invoke-Verify $d) 'versions disagree'
  } else { Write-Output 'skip mixed Microsoft runtime versions (no differing System32 msvcp140.dll on this host)' }
} finally {
  Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
}
Write-Output "RESULT passes=$script:passes failures=$script:failures"
if ($script:failures -gt 0) { exit 1 }
