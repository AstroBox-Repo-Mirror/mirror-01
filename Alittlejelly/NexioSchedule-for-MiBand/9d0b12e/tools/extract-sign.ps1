# Extract signing certs for watch rpk
# Usage: powershell -ExecutionPolicy Bypass -File tools\extract-sign.ps1
# Custom keystore:
#   .\tools\extract-sign.ps1 -Keystore "E:\path\your.jks" -StorePass "pass" -KeyAlias "alias" -KeyPass "pass"

param(
    [string]$Keystore = "C:\Users\18978\.android\debug.keystore",
    [string]$StorePass = "android",
    [string]$KeyAlias = "androiddebugkey",
    [string]$KeyPass = "android",
    [string]$OutDir = "D:\mimoProject\sign"
)

$ErrorActionPreference = "Stop"
$openssl = "C:\Program Files\Git\usr\bin\openssl.exe"
if (-not (Test-Path $openssl)) { $openssl = "openssl" }
$tmp = Join-Path $env:TEMP ("sign-extract-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force $tmp | Out-Null

$p12 = Join-Path $tmp "store.p12"
$pem = Join-Path $tmp "keystore.pem"

Write-Host "[1/3] keystore -> p12"
keytool -importkeystore -srckeystore $Keystore -destkeystore $p12 -srcstoretype jks -deststoretype pkcs12 -srcstorepass $StorePass -deststorepass $StorePass -srcalias $KeyAlias -destalias $KeyAlias -srckeypass $KeyPass -destkeypass $KeyPass -noprompt

Write-Host "[2/3] p12 -> pem"
& $openssl pkcs12 -nodes -in $p12 -out $pem -passin pass:$StorePass

Write-Host "[3/3] split private/certificate"
$lines = Get-Content $pem
$priv = New-Object System.Collections.Generic.List[string]
$cert = New-Object System.Collections.Generic.List[string]
$mode = ""
foreach ($line in $lines) {
    if ($line -match 'BEGIN (RSA )?PRIVATE KEY') { $mode = "priv" }
    if ($line -match 'BEGIN CERTIFICATE') { $mode = "cert" }
    if ($mode -eq "priv") { $priv.Add($line) }
    if ($mode -eq "cert") { $cert.Add($line) }
    if ($line -match 'END (RSA )?PRIVATE KEY') { $mode = "" }
    if ($line -match 'END CERTIFICATE') { $mode = "" }
}

foreach ($target in @("debug", "release")) {
    $dir = Join-Path $OutDir $target
    New-Item -ItemType Directory -Force $dir | Out-Null
    Set-Content -Path (Join-Path $dir "private.pem") -Value $priv -Encoding ascii
    Set-Content -Path (Join-Path $dir "certificate.pem") -Value $cert -Encoding ascii
    Write-Host ("wrote " + $dir)
}

Remove-Item -Recurse -Force $tmp
Write-Host "Done. Run npm run build to pack rpk."
