<#
.SYNOPSIS
  Shared Microsoft Authenticode identity check for app-local VC++ runtime DLLs.
.DESCRIPTION
  Dot-sourced by stage-windows-msvc-runtime.ps1 and verify-windows-msvc-runtime.ps1.
  Windows PowerShell 5.1 compatible so a pristine Windows image can run it.
  VS 2022 14.44 redistributables are signed as "Microsoft Windows" (CRT) and
  "Microsoft Windows Software Compatibility Publisher" (OpenMP), not
  "Microsoft Corporation", so a CN substring test is both wrong and weak.
  Require: Authenticode Valid, an exact Microsoft signer subject, a
  Microsoft-named issuer, and - whenever the chain can be resolved to a
  self-signed root (online, cached, or from embedded certificates) - a pinned
  Microsoft root (Root CA 2010 or 2011).
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
  if ($cert.Issuer -notmatch '^CN=Microsoft [^,]+, (.+, )?O=Microsoft Corporation(,|$)') {
    throw "Runtime signer issued by a non-Microsoft CA: $leaf ($($cert.Issuer))"
  }
  # Authenticode Valid (WinVerifyTrust) already validated a full trusted chain
  # and the timestamp. Re-build it only to identify the root, adding the
  # certificates embedded in the file so this also works on an offline host.
  # The reported signer may come from a Windows catalog whose intermediate is
  # neither embedded nor cached offline; then the chain cannot reach a root
  # here and the WinVerifyTrust result plus exact Microsoft subject/issuer
  # identity is the evidence. Any chain that DOES reach a self-signed root
  # must end in a pinned Microsoft root.
  $chain = New-Object System.Security.Cryptography.X509Certificates.X509Chain
  $chain.ChainPolicy.RevocationMode = 'NoCheck'
  $chain.ChainPolicy.VerificationFlags = 'IgnoreNotTimeValid'
  try {
    $embedded = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2Collection
    $embedded.Import($Path)
    [void]$chain.ChainPolicy.ExtraStore.AddRange($embedded)
  } catch { Write-Verbose "No embedded certificates readable from $leaf" }
  [void]$chain.Build($cert)
  $elements = @($chain.ChainElements)
  $top = $elements[$elements.Count - 1].Certificate
  $script:LastChainReachedRoot = ($elements.Count -ge 2 -and $top.Subject -eq $top.Issuer)
  if ($script:LastChainReachedRoot -and $top.Thumbprint.ToUpperInvariant() -notin $script:MicrosoftRootThumbprints) {
    throw "Runtime signer does not chain to a pinned Microsoft root: $leaf ($($top.Thumbprint))"
  }
}
