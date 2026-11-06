<#
.SYNOPSIS
  Shared Microsoft Authenticode identity check for app-local VC++ runtime DLLs.
.DESCRIPTION
  Dot-sourced by stage-windows-msvc-runtime.ps1 and verify-windows-msvc-runtime.ps1.
  Windows PowerShell 5.1 compatible so a pristine Windows image can run it.
  VS 2022 14.44 redistributables are signed as "Microsoft Windows" (CRT) and
  "Microsoft Windows Software Compatibility Publisher" (OpenMP), not
  "Microsoft Corporation", so a CN substring test is both wrong and weak.
  Require: Authenticode Valid, an exact Microsoft signer subject, and a chain
  that terminates in a pinned Microsoft root (Root CA 2010 or 2011).
#>
$script:MicrosoftSignerSubjects = @(
  'CN=Microsoft Corporation, O=Microsoft Corporation, L=Redmond, S=Washington, C=US',
  'CN=Microsoft Windows, O=Microsoft Corporation, L=Redmond, S=Washington, C=US',
  'CN=Microsoft Windows Software Compatibility Publisher, O=Microsoft Corporation, L=Redmond, S=Washington, C=US'
)
$script:MicrosoftRootThumbprints = @(
  '3B1EFD3A66EA28B16697394703A72CA340A05BD5', # Microsoft Root Certificate Authority 2010
  '8F43288AD272F3103B6FB1428485EA3014C0BCFE'  # Microsoft Root Certificate Authority 2011
)

function Assert-MicrosoftSigned {
  param([Parameter(Mandatory)][string]$Path)
  $leaf = Split-Path -Leaf $Path
  $signature = Get-AuthenticodeSignature -LiteralPath $Path
  if ($signature.Status -ne 'Valid' -or $null -eq $signature.SignerCertificate) {
    throw "Unsigned or invalid Authenticode signature: $leaf ($($signature.Status))"
  }
  $cert = $signature.SignerCertificate
  if ($cert.Subject -notin $script:MicrosoftSignerSubjects) {
    throw "Non-Microsoft runtime signer: $leaf ($($cert.Subject))"
  }
  # Authenticode Valid already covered timestamped validity; build the chain
  # only to identify its root, tolerating an expired-but-timestamped leaf.
  $chain = New-Object System.Security.Cryptography.X509Certificates.X509Chain
  $chain.ChainPolicy.RevocationMode = 'NoCheck'
  $chain.ChainPolicy.VerificationFlags = 'IgnoreNotTimeValid'
  [void]$chain.Build($cert)
  $elements = @($chain.ChainElements)
  if ($elements.Count -lt 2) { throw "Microsoft signer chain incomplete: $leaf" }
  $root = $elements[$elements.Count - 1].Certificate.Thumbprint.ToUpperInvariant()
  if ($root -notin $script:MicrosoftRootThumbprints) {
    throw "Runtime signer does not chain to a pinned Microsoft root: $leaf ($root)"
  }
}
