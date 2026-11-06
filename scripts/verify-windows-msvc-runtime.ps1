#Requires -Version 7.0
<#
.SYNOPSIS
  Verify an extracted Windows GG-CORE package's app-local MSVC runtime.
.DESCRIPTION
  Verifies exact Microsoft redistribution filenames, signatures, matching
  SHA-256 manifests and packaged gg-core-cli.exe startup. A passing check
  on a developer machine is NOT a clean-Windows acceptance receipt.
#>
[CmdletBinding()]
param([Parameter(Mandatory)][string]$StageDirectory)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$stage = (Resolve-Path -LiteralPath $StageDirectory -ErrorAction Stop).Path
$manifestPath = Join-Path $stage 'MSVC_REDISTRIBUTABLES.json'
if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
  throw 'MSVC runtime provenance manifest missing from Windows package'
}
$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
if ($manifest.format_version -ne 1 -or $manifest.architecture -ne 'x64' -or
    $manifest.distribution -ne 'licensed-visual-studio-app-local-redist') {
  throw 'Unexpected app-local runtime manifest schema or architecture'
}
$required = @('msvcp140.dll', 'vcomp140.dll', 'vcruntime140.dll', 'vcruntime140_1.dll')
$listed = @($manifest.windows_runtime)
if ($listed.Count -ne $required.Count) { throw 'Unexpected number of Microsoft runtime entries' }
$seen = @{}
foreach ($item in $listed) {
  $name = [string]$item.file
  if ($name -notin $required -or $seen.ContainsKey($name)) {
    throw 'Unexpected or duplicate Microsoft runtime DLL entry'
  }
  $seen[$name] = $true
  $path = Join-Path $stage $name
  if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
    throw "Missing required packaged $name"
  }
  $sig = Get-AuthenticodeSignature -LiteralPath $path
  if ($sig.Status -ne 'Valid' -or $null -eq $sig.SignerCertificate -or
      $sig.SignerCertificate.Subject -notmatch 'CN=Microsoft Corporation') {
    throw "Microsoft signature verification failed: $name"
  }
  $actual = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
  if ($actual -ne [string]$item.sha256) {
    throw "Runtime SHA-256 mismatch: $name"
  }
}
foreach ($name in $required) {
  if (-not $seen.ContainsKey($name)) { throw "Omitted required DLL $name" }
}
$exe = Join-Path $stage 'gg-core-cli.exe'
if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) {
  throw 'Windows package has no gg-core-cli.exe'
}
$version = & $exe version --json
if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($version)) {
  throw 'Packaged CLI could not launch; runtime dependency closure unverified'
}
$meta = $version | ConvertFrom-Json
if ('gguf' -notin @($meta.features)) { throw 'Packaged CLI lacks required real GGUF backend' }
Write-Output "PASS: Windows app-local MSVC runtime signatures/hashes and CLI startup"
