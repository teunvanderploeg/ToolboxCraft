param([Parameter(Mandatory = $true)][string]$Path)
$ErrorActionPreference = 'Stop'
$tcSignature = Get-AuthenticodeSignature -LiteralPath $Path
if ($tcSignature.Status -ne 'Valid') {
    throw "Invalid or untrusted Authenticode signature on $Path ($($tcSignature.Status))"
}
if (-not $tcSignature.TimeStamperCertificate) {
    throw "Missing trusted timestamp on $Path"
}
Write-Host "Verified signature: $Path; publisher: $($tcSignature.SignerCertificate.Subject)"
