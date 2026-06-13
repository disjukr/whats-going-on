#Requires -Version 5.1

param(
  [string]$Version,
  [string]$OutDir,
  [string]$PackageIdentityName = "Disjukr.WhatsGoingOn",
  [string]$Publisher = "CN=Whats Going On Dev",
  [string]$PublisherDisplayName = "disjukr",
  [string]$CertificatePath,
  [string]$CertificatePassword,
  [switch]$SkipBuild,
  [switch]$SkipSign
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

function ConvertTo-MsixVersion {
  param([string]$CargoVersion)

  $numericVersion = ($CargoVersion -split '[-+]')[0]
  $parts = @($numericVersion -split '\.')
  if ($parts.Count -gt 4) {
    throw "MSIX versions can contain at most four numeric parts: $CargoVersion"
  }
  while ($parts.Count -lt 4) {
    $parts += "0"
  }
  foreach ($part in $parts) {
    if ($part -notmatch '^\d+$') {
      throw "MSIX version parts must be numeric: $CargoVersion"
    }
    $partNumber = [int]$part
    if ($partNumber -lt 0 -or $partNumber -gt 65535) {
      throw "MSIX version part is out of range 0..65535: $CargoVersion"
    }
  }
  return ($parts -join ".")
}

function Find-WindowsSdkTool {
  param([string]$Name)

  $command = Get-Command $Name -ErrorAction SilentlyContinue
  if ($command) {
    return $command.Source
  }

  $kitsRoot = Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10\bin"
  if (-not (Test-Path -LiteralPath $kitsRoot)) {
    throw "Windows SDK bin directory was not found: $kitsRoot"
  }
  $tool = Get-ChildItem -LiteralPath $kitsRoot -Recurse -Filter $Name |
    Where-Object { $_.FullName -match '\\x64\\' } |
    Sort-Object FullName -Descending |
    Select-Object -First 1
  if (-not $tool) {
    throw "$Name was not found. Install the Windows SDK or run from a Developer PowerShell."
  }
  return $tool.FullName
}

function ConvertTo-XmlEscapedText {
  param([string]$Value)

  return [System.Security.SecurityElement]::Escape($Value)
}

function New-PngIcon {
  param(
    [string]$SourceIcon,
    [string]$Destination,
    [int]$Size
  )

  Add-Type -AssemblyName System.Drawing
  $icon = [System.Drawing.Icon]::ExtractAssociatedIcon($SourceIcon)
  if (-not $icon) {
    throw "Could not load icon: $SourceIcon"
  }
  $bitmap = New-Object System.Drawing.Bitmap $Size, $Size
  $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
  try {
    $graphics.Clear([System.Drawing.Color]::Transparent)
    $graphics.DrawIcon($icon, (New-Object System.Drawing.Rectangle 0, 0, $Size, $Size))
    $bitmap.Save($Destination, [System.Drawing.Imaging.ImageFormat]::Png)
  } finally {
    $graphics.Dispose()
    $bitmap.Dispose()
    $icon.Dispose()
  }
}

function New-AppxManifest {
  param(
    [string]$Path,
    [string]$IdentityName,
    [string]$IdentityPublisher,
    [string]$IdentityVersion,
    [string]$DisplayPublisher
  )

  $identityName = ConvertTo-XmlEscapedText $IdentityName
  $identityPublisher = ConvertTo-XmlEscapedText $IdentityPublisher
  $identityVersion = ConvertTo-XmlEscapedText $IdentityVersion
  $displayPublisher = ConvertTo-XmlEscapedText $DisplayPublisher

  $manifest = @"
<?xml version="1.0" encoding="utf-8"?>
<Package
  xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
  xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"
  xmlns:desktop="http://schemas.microsoft.com/appx/manifest/desktop/windows10"
  xmlns:desktop7="http://schemas.microsoft.com/appx/manifest/desktop/windows10/7"
  xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities"
  IgnorableNamespaces="uap desktop desktop7 rescap">
  <Identity
    Name="$identityName"
    Publisher="$identityPublisher"
    Version="$identityVersion"
    ProcessorArchitecture="x64" />
  <Properties>
    <DisplayName>Whats Going On</DisplayName>
    <PublisherDisplayName>$displayPublisher</PublisherDisplayName>
    <Logo>Assets\StoreLogo.png</Logo>
  </Properties>
  <Dependencies>
    <TargetDeviceFamily
      Name="Windows.Desktop"
      MinVersion="10.0.22000.0"
      MaxVersionTested="10.0.26100.0" />
  </Dependencies>
  <Resources>
    <Resource Language="en-us" />
  </Resources>
  <Applications>
    <Application
      Id="WgoUser"
      Executable="wgo-windows-user.exe"
      EntryPoint="Windows.FullTrustApplication">
      <uap:VisualElements
        DisplayName="Whats Going On"
        Description="Remote machine explorer and daemon tray"
        BackgroundColor="transparent"
        Square44x44Logo="Assets\Square44x44Logo.png"
        Square150x150Logo="Assets\Square150x150Logo.png" />
      <Extensions>
        <desktop:Extension
          Category="windows.startupTask"
          Executable="wgo-windows-user.exe"
          EntryPoint="Windows.FullTrustApplication">
          <desktop:StartupTask
            TaskId="WgoUserTray"
            Enabled="true"
            DisplayName="Whats Going On" />
        </desktop:Extension>
        <desktop7:Extension
          Category="windows.service"
          Executable="wgo-windows-system.exe"
          EntryPoint="Windows.FullTrustApplication">
          <desktop7:Service
            Name="wgo-windows-system"
            StartupType="delayedStart"
            StartAccount="localSystem"
            Arguments="service run" />
        </desktop7:Extension>
      </Extensions>
    </Application>
  </Applications>
  <Capabilities>
    <rescap:Capability Name="runFullTrust" />
    <rescap:Capability Name="packagedServices" />
    <rescap:Capability Name="localSystemServices" />
  </Capabilities>
</Package>
"@
  Set-Content -LiteralPath $Path -Value $manifest -Encoding UTF8
}

function Get-DevCertificate {
  param([string]$Subject)

  $cert = Get-ChildItem Cert:\CurrentUser\My |
    Where-Object {
      $_.Subject -eq $Subject -and
      $_.HasPrivateKey -and
      $_.NotAfter -gt (Get-Date) -and
      ($_.EnhancedKeyUsageList | Where-Object { $_.ObjectId -eq "1.3.6.1.5.5.7.3.3" })
    } |
    Sort-Object NotAfter -Descending |
    Select-Object -First 1

  if ($cert) {
    return $cert
  }

  return New-SelfSignedCertificate `
    -Type CodeSigningCert `
    -Subject $Subject `
    -KeyAlgorithm RSA `
    -KeyLength 2048 `
    -CertStoreLocation Cert:\CurrentUser\My `
    -NotAfter (Get-Date).AddYears(2)
}

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
if (-not $Version) {
  $Version = Get-CargoPackageVersion (Join-Path $RepoRoot "daemon\windows\Cargo.toml")
}
$MsixVersion = ConvertTo-MsixVersion $Version
if (-not $OutDir) {
  $OutDir = Join-Path $RepoRoot "dist\windows"
}

$MakeAppx = Find-WindowsSdkTool "makeappx.exe"
$SignTool = $null
if (-not $SkipSign) {
  $SignTool = Find-WindowsSdkTool "signtool.exe"
}

$PackageBaseName = "wgo-windows-daemon-$Version"
$StagingDir = Join-Path $OutDir "$PackageBaseName-msix"
if ($SkipSign) {
  $MsixPath = Join-Path $OutDir "$PackageBaseName.unsigned.msix"
} else {
  $MsixPath = Join-Path $OutDir "$PackageBaseName.msix"
}
$ReleaseDir = Join-Path $RepoRoot "target\release"
$SystemExe = Join-Path $ReleaseDir "wgo-windows-system.exe"
$UserExe = Join-Path $ReleaseDir "wgo-windows-user.exe"

if (-not $SkipBuild) {
  Push-Location $RepoRoot
  try {
    cargo build -p wgo-windows-daemon --release --bins
  } finally {
    Pop-Location
  }
}

if (-not (Test-Path -LiteralPath $SystemExe)) {
  throw "Missing release binary: $SystemExe"
}
if (-not (Test-Path -LiteralPath $UserExe)) {
  throw "Missing release binary: $UserExe"
}

if (Test-Path -LiteralPath $StagingDir) {
  Remove-Item -LiteralPath $StagingDir -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $StagingDir | Out-Null
$AssetsDir = Join-Path $StagingDir "Assets"
New-Item -ItemType Directory -Force -Path $AssetsDir | Out-Null

Copy-Item -LiteralPath $SystemExe -Destination (Join-Path $StagingDir "wgo-windows-system.exe")
Copy-Item -LiteralPath $UserExe -Destination (Join-Path $StagingDir "wgo-windows-user.exe")

$IconPath = Join-Path $RepoRoot "daemon\windows\assets\tray.ico"
if (-not (Test-Path -LiteralPath $IconPath)) {
  throw "Missing tray icon: $IconPath"
}
New-PngIcon -SourceIcon $IconPath -Destination (Join-Path $AssetsDir "Square44x44Logo.png") -Size 44
New-PngIcon -SourceIcon $IconPath -Destination (Join-Path $AssetsDir "Square150x150Logo.png") -Size 150
New-PngIcon -SourceIcon $IconPath -Destination (Join-Path $AssetsDir "StoreLogo.png") -Size 50

New-AppxManifest `
  -Path (Join-Path $StagingDir "AppxManifest.xml") `
  -IdentityName $PackageIdentityName `
  -IdentityPublisher $Publisher `
  -IdentityVersion $MsixVersion `
  -DisplayPublisher $PublisherDisplayName

if (Test-Path -LiteralPath $MsixPath) {
  Remove-Item -LiteralPath $MsixPath -Force
}
& $MakeAppx pack /d $StagingDir /p $MsixPath /overwrite | Write-Host
if ($LASTEXITCODE -ne 0) {
  throw "MakeAppx failed with exit code $LASTEXITCODE"
}

if (-not $SkipSign) {
  if ($CertificatePath) {
    $signArgs = @("sign", "/fd", "SHA256", "/f", $CertificatePath)
    if ($CertificatePassword) {
      $signArgs += @("/p", $CertificatePassword)
    }
    $signArgs += $MsixPath
    & $SignTool @signArgs | Write-Host
  } else {
    $cert = Get-DevCertificate $Publisher
    & $SignTool sign /fd SHA256 /sha1 $cert.Thumbprint $MsixPath | Write-Host
    $certPath = Join-Path $OutDir "$PackageBaseName.cer"
    Export-Certificate -Cert $cert -FilePath $certPath -Force | Out-Null
    Write-Host "Wrote development certificate: $certPath"
    Write-Host "Trust the certificate before installing this development MSIX."
  }
  if ($LASTEXITCODE -ne 0) {
    throw "SignTool failed with exit code $LASTEXITCODE"
  }
}

Write-Host "Wrote MSIX: $MsixPath"
Write-Host "Staging directory: $StagingDir"
