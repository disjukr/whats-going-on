#Requires -Version 5.1

param(
  [string]$Version,
  [string]$OutDir,
  [string]$Manufacturer = "JongChan Choi",
  [switch]$SkipBuild
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

function ConvertTo-MsiVersion {
  param([string]$CargoVersion)

  $numericVersion = ($CargoVersion -split '[-+]')[0]
  $parts = @($numericVersion -split '\.')
  if ($parts.Count -gt 3) {
    throw "MSI product versions can contain at most three numeric parts: $CargoVersion"
  }
  while ($parts.Count -lt 3) {
    $parts += "0"
  }
  for ($index = 0; $index -lt $parts.Count; $index++) {
    $part = $parts[$index]
    if ($part -notmatch '^\d+$') {
      throw "MSI version parts must be numeric: $CargoVersion"
    }
    $partNumber = [int]$part
    $max = if ($index -lt 2) { 255 } else { 65535 }
    if ($partNumber -lt 0 -or $partNumber -gt $max) {
      throw "MSI version part $($index + 1) is out of range 0..$max`: $CargoVersion"
    }
  }
  return ($parts -join ".")
}

function ConvertTo-XmlEscapedText {
  param([string]$Value)

  return [System.Security.SecurityElement]::Escape($Value)
}

function ConvertTo-WixSourcePath {
  param([string]$Path)

  return (ConvertTo-XmlEscapedText ((Resolve-Path -LiteralPath $Path).Path))
}

function Test-DotnetSdkAvailable {
  $dotnet = Get-Command dotnet -ErrorAction SilentlyContinue
  if (-not $dotnet) {
    return $false
  }
  $sdks = & $dotnet.Source --list-sdks 2>$null
  return $LASTEXITCODE -eq 0 -and $sdks
}

function Invoke-Wix {
  param(
    [string]$RepoRoot,
    [string[]]$Arguments
  )

  $wix = Get-Command wix -ErrorAction SilentlyContinue
  if ($wix) {
    & $wix.Source @Arguments
    return
  }

  if (-not (Test-DotnetSdkAvailable)) {
    throw "WiX CLI was not found. Install WiX on PATH, or install the .NET SDK and run 'dotnet tool restore' so this repo's local wix tool can run."
  }

  Push-Location $RepoRoot
  try {
    & dotnet tool restore
    if ($LASTEXITCODE -ne 0) {
      throw "dotnet tool restore failed with exit code $LASTEXITCODE"
    }
    & dotnet tool run wix -- @Arguments
  } finally {
    Pop-Location
  }
}

function Add-WixExtension {
  param(
    [string]$RepoRoot,
    [string]$Extension
  )

  Invoke-Wix -RepoRoot $RepoRoot -Arguments @(
    "extension",
    "add",
    $Extension
  )
  if ($LASTEXITCODE -ne 0) {
    throw "Failed to add WiX extension $Extension with exit code $LASTEXITCODE"
  }
}

function New-DaemonMsiSource {
  param(
    [string]$Path,
    [string]$Version,
    [string]$Manufacturer,
    [string]$SystemExe,
    [string]$UserExe,
    [string]$Icon
  )

  $versionText = ConvertTo-XmlEscapedText $Version
  $manufacturerText = ConvertTo-XmlEscapedText $Manufacturer
  $systemExePath = ConvertTo-WixSourcePath $SystemExe
  $userExePath = ConvertTo-WixSourcePath $UserExe
  $iconPath = ConvertTo-WixSourcePath $Icon

  $source = @"
<Wix
  xmlns="http://wixtoolset.org/schemas/v4/wxs">
  <Package
    Name="Whats Going On Daemon"
    Manufacturer="$manufacturerText"
    Version="$versionText"
    UpgradeCode="{B7314DA6-AE47-45BB-BC82-C35F06A44FD8}"
    Scope="perMachine">
    <MajorUpgrade DowngradeErrorMessage="A newer version of [ProductName] is already installed." />
    <MediaTemplate EmbedCab="yes" />

    <Icon Id="WgoTrayIcon.ico" SourceFile="$iconPath" />
    <Property Id="ARPPRODUCTICON" Value="WgoTrayIcon.ico" />

    <Launch Condition="Privileged" Message="[ProductName] installs a LocalSystem service and must be installed with administrator privileges." />
    <Property Id="WIXUI_EXITDIALOGOPTIONALTEXT" Value="Whats Going On Daemon was installed successfully." />
    <Property Id="ARPNOMODIFY" Value="1" />

    <UI Id="WgoInstallUI">
      <TextStyle Id="WixUI_Font_Normal" FaceName="Tahoma" Size="8" />
      <TextStyle Id="WixUI_Font_Bigger" FaceName="Tahoma" Size="12" />
      <TextStyle Id="WixUI_Font_Title" FaceName="Tahoma" Size="9" Bold="yes" />
      <Property Id="DefaultUIFont" Value="WixUI_Font_Normal" />

      <DialogRef Id="ErrorDlg" />
      <DialogRef Id="FatalError" />
      <DialogRef Id="FilesInUse" />
      <DialogRef Id="MsiRMFilesInUse" />
      <DialogRef Id="PrepareDlg" />
      <DialogRef Id="ProgressDlg" />
      <DialogRef Id="ResumeDlg" />
      <DialogRef Id="UserExit" />
      <DialogRef Id="WelcomeDlg" />
      <DialogRef Id="VerifyReadyDlg" />

      <Publish Dialog="ExitDialog" Control="Finish" Event="EndDialog" Value="Return" Order="999" />
      <Publish Dialog="WelcomeDlg" Control="Next" Event="NewDialog" Value="VerifyReadyDlg" Condition="NOT Installed OR PATCH" />
      <Publish Dialog="VerifyReadyDlg" Control="Back" Event="NewDialog" Value="WelcomeDlg" Condition="NOT Installed OR PATCH" />
    </UI>
    <UIRef Id="WixUI_Common" />

    <SetProperty
      Id="WixUnelevatedShellExecTarget"
      Value="[#UserTrayExe]"
      Before="LaunchWgoTrayApp"
      Sequence="execute"
      Condition="NOT Installed" />
    <CustomAction
      Id="LaunchWgoTrayApp"
      BinaryRef="Wix4UtilCA_`$(sys.BUILDARCHSHORT)"
      DllEntry="WixUnelevatedShellExec"
      Execute="immediate"
      Return="ignore" />
    <InstallExecuteSequence>
      <Custom Action="LaunchWgoTrayApp" After="InstallFinalize" Condition="NOT Installed" />
    </InstallExecuteSequence>

    <StandardDirectory Id="ProgramFiles64Folder">
      <Directory Id="INSTALLFOLDER" Name="WhatsGoingOn">
        <Component Id="SystemDaemonComponent" Guid="{2BCB9D17-4623-4E75-962C-EAC4C82C9D8E}" Bitness="always64">
          <File Id="SystemDaemonExe" Source="$systemExePath" KeyPath="yes" />
          <ServiceInstall
            Id="WgoSystemService"
            Name="wgo-windows-system"
            DisplayName="Whats Going On System Daemon"
            Description="Runs the Whats Going On Windows system daemon."
            Type="ownProcess"
            Start="auto"
            ErrorControl="normal"
            Vital="yes"
            Arguments="service run --config &quot;[CommonAppDataFolder]WhatsGoingOn\wgo.yaml&quot;" />
          <ServiceControl
            Id="WgoSystemServiceControl"
            Name="wgo-windows-system"
            Start="install"
            Stop="both"
            Remove="uninstall"
            Wait="yes" />
        </Component>

        <Component Id="UserTrayComponent" Guid="{6F43D08E-426E-4F73-A947-06987D99CDA3}" Bitness="always64">
          <File Id="UserTrayExe" Source="$userExePath" KeyPath="yes" />
          <RegistryValue
            Root="HKLM"
            Key="Software\Microsoft\Windows\CurrentVersion\Run"
            Name="Whats Going On"
            Type="string"
            Value="&quot;[INSTALLFOLDER]wgo-windows-user.exe&quot;" />
        </Component>
      </Directory>
    </StandardDirectory>

    <StandardDirectory Id="ProgramMenuFolder">
      <Directory Id="ProgramMenuAppFolder" Name="Whats Going On">
        <Component Id="StartMenuShortcutComponent" Guid="{60994307-B63F-4488-BD16-20196C11EED1}" Bitness="always64">
          <Shortcut
            Id="StartMenuShortcut"
            Name="Whats Going On"
            Target="[INSTALLFOLDER]wgo-windows-user.exe"
            WorkingDirectory="INSTALLFOLDER"
            Icon="WgoTrayIcon.ico" />
          <RemoveFolder Id="RemoveProgramMenuAppFolder" On="uninstall" />
          <RegistryValue
            Root="HKLM"
            Key="Software\WhatsGoingOn\Daemon"
            Name="StartMenuShortcut"
            Type="integer"
            Value="1"
            KeyPath="yes" />
        </Component>
      </Directory>
    </StandardDirectory>

    <Feature Id="MainFeature" Title="Whats Going On Daemon" Level="1">
      <ComponentRef Id="SystemDaemonComponent" />
      <ComponentRef Id="UserTrayComponent" />
      <ComponentRef Id="StartMenuShortcutComponent" />
    </Feature>
  </Package>
</Wix>
"@

  Set-Content -LiteralPath $Path -Value $source -Encoding UTF8
}

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
if (-not $Version) {
  $Version = Get-CargoPackageVersion (Join-Path $RepoRoot "daemon\windows\Cargo.toml")
}
$MsiVersion = ConvertTo-MsiVersion $Version
if (-not $OutDir) {
  $OutDir = Join-Path $RepoRoot "dist\windows"
}

$PackageBaseName = "wgo-windows-daemon-$Version"
$StagingDir = Join-Path $OutDir "$PackageBaseName-msi"
$MsiSourcePath = Join-Path $StagingDir "Package.wxs"
$MsiPath = Join-Path $OutDir "$PackageBaseName.msi"
$ReleaseDir = Join-Path $RepoRoot "target\release"
$SystemExe = Join-Path $ReleaseDir "wgo-windows-system.exe"
$UserExe = Join-Path $ReleaseDir "wgo-windows-user.exe"
$Icon = Join-Path $RepoRoot "daemon\windows\assets\tray.ico"

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
if (-not (Test-Path -LiteralPath $Icon)) {
  throw "Missing tray icon: $Icon"
}

if (Test-Path -LiteralPath $StagingDir) {
  Remove-Item -LiteralPath $StagingDir -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $StagingDir | Out-Null

New-DaemonMsiSource `
  -Path $MsiSourcePath `
  -Version $MsiVersion `
  -Manufacturer $Manufacturer `
  -SystemExe $SystemExe `
  -UserExe $UserExe `
  -Icon $Icon

if (Test-Path -LiteralPath $MsiPath) {
  Remove-Item -LiteralPath $MsiPath -Force
}

Add-WixExtension -RepoRoot $RepoRoot -Extension "WixToolset.UI.wixext/7.0.0"
Add-WixExtension -RepoRoot $RepoRoot -Extension "WixToolset.Util.wixext/7.0.0"

Invoke-Wix -RepoRoot $RepoRoot -Arguments @(
  "build",
  "-ext",
  "WixToolset.UI.wixext",
  "-ext",
  "WixToolset.Util.wixext",
  $MsiSourcePath,
  "-arch",
  "x64",
  "-o",
  $MsiPath
)
if ($LASTEXITCODE -ne 0) {
  throw "WiX failed with exit code $LASTEXITCODE"
}

Write-Host "Wrote MSI: $MsiPath"
Write-Host "Staging directory: $StagingDir"
