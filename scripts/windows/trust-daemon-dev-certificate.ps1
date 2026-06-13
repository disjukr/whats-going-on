#Requires -Version 5.1

param(
  [string]$Version,
  [string]$OutDir,
  [string]$CertificatePath,
  [switch]$NoElevate
)

$ErrorActionPreference = "Stop"

function Get-CargoPackageVersion {
  param([string]$CargoTomlPath)

  $cargoToml = Get-Content -LiteralPath $CargoTomlPath
  $versionLine = $cargoToml |
    Where-Object { $_ -match '^version\s*=\s*"([^"]+)"' } |
    Select-Object -First 1
  if (-not $versionLine) {
    throw "Could not infer daemon version from $CargoTomlPath"
  }
  return [regex]::Match($versionLine, '^version\s*=\s*"([^"]+)"').Groups[1].Value
}

function Test-IsAdministrator {
  $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
  $principal = New-Object Security.Principal.WindowsPrincipal $identity
  return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function ConvertTo-PowerShellLiteral {
  param([string]$Value)

  return "'" + $Value.Replace("'", "''") + "'"
}

function Invoke-ElevatedSelf {
  $commandParts = @("&", (ConvertTo-PowerShellLiteral $PSCommandPath), "-NoElevate")
  if ($Version) {
    $commandParts += @("-Version", (ConvertTo-PowerShellLiteral $Version))
  }
  if ($OutDir) {
    $commandParts += @("-OutDir", (ConvertTo-PowerShellLiteral $OutDir))
  }
  if ($CertificatePath) {
    $commandParts += @("-CertificatePath", (ConvertTo-PowerShellLiteral $CertificatePath))
  }

  $command = $commandParts -join " "
  $encodedCommand = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($command))
  $process = Start-Process `
    -FilePath powershell.exe `
    -Verb RunAs `
    -Wait `
    -PassThru `
    -ArgumentList @(
      "-NoProfile",
      "-ExecutionPolicy",
      "Bypass",
      "-EncodedCommand",
      $encodedCommand
    )
  exit $process.ExitCode
}

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
if (-not $Version) {
  $Version = Get-CargoPackageVersion (Join-Path $RepoRoot "daemon\windows\Cargo.toml")
}
if (-not $OutDir) {
  $OutDir = Join-Path $RepoRoot "dist\windows"
}
if (-not $CertificatePath) {
  $CertificatePath = Join-Path $OutDir "wgo-windows-daemon-$Version.cer"
}

if (-not (Test-Path -LiteralPath $CertificatePath)) {
  throw "Missing development certificate: $CertificatePath. Run 'deno task windows:package:daemon' first."
}

if (-not (Test-IsAdministrator)) {
  if ($NoElevate) {
    throw "Administrator privileges are required to trust the development certificate."
  }
  Write-Host "Administrator privileges are required. Requesting elevation..."
  Invoke-ElevatedSelf
}

$certificate = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2 $CertificatePath
$storePath = "Cert:\LocalMachine\TrustedPeople"
$existing = Get-ChildItem -LiteralPath $storePath |
  Where-Object { $_.Thumbprint -eq $certificate.Thumbprint } |
  Select-Object -First 1

if ($existing) {
  Write-Host "Development certificate is already trusted: $($certificate.Thumbprint)"
  exit 0
}

Import-Certificate -FilePath $CertificatePath -CertStoreLocation $storePath | Out-Null
Write-Host "Trusted development certificate: $($certificate.Thumbprint)"
