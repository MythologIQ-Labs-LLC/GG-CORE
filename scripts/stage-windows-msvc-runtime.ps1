#Requires -Version 7.0
<#
.SYNOPSIS
  Stage only verified, redistributable Microsoft x64 VC++/OpenMP DLLs.
.DESCRIPTION
  GG-CORE Windows portable packages must launch without a preinstalled
  VC++ runtime. This tool uses licensed Visual Studio redistribution
  directories, never System32. It validates signatures and full hashes
  before and after copying, and never downloads or changes a Microsoft DLL.
  Licensing: https://learn.microsoft.com/cpp/windows/redistributing-visual-cpp-files
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory)][string]$StageDirectory,
  [string]$RedistRoot
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$required = [ordered]@{
  'msvcp140.dll' = 'CRT'
  'vcomp140.dll' = 'OpenMP'
  'vcruntime140.dll' = 'CRT'
  'vcruntime140_1.dll' = 'CRT'
}
function Find-RedistDirectory {
  param([string]$Explicit)
  if (-not [string]::IsNullOrWhiteSpace($Explicit)) {
    return (Resolve-Path -LiteralPath $Explicit -ErrorAction Stop).Path
  }
  if (-not [string]::IsNullOrWhiteSpace($env:VCToolsRedistDir)) {
    return (Resolve-Path -LiteralPath $env:VCToolsRedistDir -ErrorAction Stop).Path
  }
  $programFilesX86 = [Environment]::GetFolderPath('ProgramFilesX86')
  $vswhere = Join-Path $programFilesX86 'Microsoft Visual Studio\Installer\vswhere.exe'
  if (-not (Test-Path -LiteralPath $vswhere -PathType Leaf)) {
    throw 'Neither VCToolsRedistDir nor installed Visual Studio redistributables found'
  }
  $install = & $vswhere -latest -products '*' -property installationPath
  if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($install)) {
    throw 'Cannot resolve a licensed Visual Studio installation'
  }
  $versions = Join-Path $install 'VC\Redist\MSVC'
  $dirs = @(Get-ChildItem -LiteralPath $versions -Directory -ErrorAction Stop |
    Sort-Object { try { [version]$_.Name } catch { [version]'0.0' } } -Descending)
  if ($dirs.Count -eq 0) { throw 'No Visual Studio MSVC redistribution versions found' }
  return $dirs[0].FullName
}
function Assert-MicrosoftSigned {
  param([string]$Path)
  $signature = Get-AuthenticodeSignature -LiteralPath $Path
  if ($signature.Status -ne 'Valid' -or
      $null -eq $signature.SignerCertificate -or
      $signature.SignerCertificate.Subject -notmatch 'CN=Microsoft Corporation') {
    throw "Unsigned or non-Microsoft runtime: $(Split-Path -Leaf $Path)"
  }
}
$root = Find-RedistDirectory -Explicit $RedistRoot
if (-not (Test-Path -LiteralPath (Join-Path $root 'x64') -PathType Container)) {
  throw 'Required licensed x64 redistributable root not present'
}
$stage = (Resolve-Path -LiteralPath $StageDirectory -ErrorAction Stop).Path
if (-not (Test-Path -LiteralPath $stage -PathType Container)) {
  throw 'Staging directory must exist'
}
if ($stage.StartsWith($root, [System.StringComparison]::OrdinalIgnoreCase)) {
  throw 'Cannot stage files into the Visual Studio redistribution source'
}
$records = @()
foreach ($item in $required.GetEnumerator()) {
  $name = $item.Key
  $family = $item.Value
  $families = @(Get-ChildItem -LiteralPath (Join-Path $root 'x64') -Directory |
    Where-Object { $_.Name -match "^Microsoft\.VC[0-9]+\.$family$" })
  if ($families.Count -ne 1) {
    throw "Expected one licensed x64 Microsoft.VCxxx.$family folder, got $($families.Count)"
  }
  $sources = @(Get-ChildItem -LiteralPath $families[0].FullName -Filter $name -File)
  if ($sources.Count -ne 1) {
    throw "Missing or ambiguous Microsoft redistributable: $name"
  }
  $source = $sources[0].FullName
  Assert-MicrosoftSigned -Path $source
  $dest = Join-Path $stage $name
  if (Test-Path -LiteralPath $dest) { throw "Staged $name already exists" }
  $before = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant()
  Copy-Item -LiteralPath $source -Destination $dest -ErrorAction Stop
  Assert-MicrosoftSigned -Path $dest
  $after = (Get-FileHash -LiteralPath $dest -Algorithm SHA256).Hash.ToLowerInvariant()
  if ($before -ne $after) {
    Remove-Item -LiteralPath $dest -ErrorAction SilentlyContinue
    throw "Staged runtime identity mismatch: $name"
  }
  $records += [ordered]@{
    file = $name
    sha256 = $after
    version = (Get-Item -LiteralPath $source).VersionInfo.FileVersion
    family = $family
    publisher = 'Microsoft Corporation'
  }
}
$manifest = [ordered]@{
  format_version = 1
  distribution = 'licensed-visual-studio-app-local-redist'
  architecture = 'x64'
  windows_runtime = @($records)
  license_reference = 'https://learn.microsoft.com/cpp/windows/redistributing-visual-cpp-files'
  source_version = (Split-Path -Leaf ($root.TrimEnd('\')))
}
$manifestPath = Join-Path $stage 'MSVC_REDISTRIBUTABLES.json'
if (Test-Path -LiteralPath $manifestPath) { throw 'Provenance manifest already exists' }
$manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $manifestPath -Encoding utf8NoBOM
Write-Output "PASS: staged $($records.Count) original signed Microsoft runtime DLLs"
