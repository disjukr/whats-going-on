param(
  [string]$Listen = "0.0.0.0:9012",
  [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$TmpDir = Join-Path $RepoRoot "tmp\dev"
$LogDir = Join-Path $RepoRoot "tmp\log"
$ConfigPath = Join-Path $TmpDir "system-wgo.yaml"
$SystemPidFile = Join-Path $TmpDir "system.pid"
$UserPidFile = Join-Path $TmpDir "user.pid"
$SystemOutLog = Join-Path $LogDir "system.out.log"
$SystemErrLog = Join-Path $LogDir "system.err.log"
$UserOutLog = Join-Path $LogDir "user.out.log"
$UserErrLog = Join-Path $LogDir "user.err.log"
$SystemExe = Join-Path $RepoRoot "target\debug\wgo-windows-system.exe"
$UserExe = Join-Path $RepoRoot "target\debug\wgo-windows-user.exe"

New-Item -ItemType Directory -Force -Path $TmpDir | Out-Null
New-Item -ItemType Directory -Force -Path $LogDir | Out-Null

function Stop-ProcessTree {
  param(
    [int]$ProcessId,
    [string]$Label = "process"
  )

  $process = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
  if ($null -eq $process) {
    return
  }

  Write-Host "Stopping $Label pid=$ProcessId"
  $output = & taskkill.exe /PID $ProcessId /T /F 2>&1
  if ($LASTEXITCODE -ne 0) {
    Write-Warning "taskkill failed for $Label pid=$ProcessId`: $($output -join ' ')"
    Stop-Process -Id $ProcessId -Force -ErrorAction SilentlyContinue
  }

  Wait-Process -Id $ProcessId -Timeout 5 -ErrorAction SilentlyContinue
}

function Test-DevDaemonProcess {
  param(
    [Microsoft.Management.Infrastructure.CimInstance]$Process,
    [string]$ExecutablePath,
    [string]$RequiredCommandLinePart
  )

  if (-not $Process.ExecutablePath -or -not $Process.CommandLine) {
    return $false
  }

  $actualPath = [System.IO.Path]::GetFullPath($Process.ExecutablePath)
  $expectedPath = [System.IO.Path]::GetFullPath($ExecutablePath)
  if (-not [string]::Equals($actualPath, $expectedPath, [System.StringComparison]::OrdinalIgnoreCase)) {
    return $false
  }

  if ($Process.CommandLine -notmatch '(^|\s)run($|\s)') {
    return $false
  }

  if ($RequiredCommandLinePart -and $Process.CommandLine.IndexOf($RequiredCommandLinePart, [System.StringComparison]::OrdinalIgnoreCase) -lt 0) {
    return $false
  }

  return $true
}

function Stop-PreviousDaemon {
  param(
    [string]$Label,
    [string]$ExecutablePath,
    [string]$PidFile,
    [string]$RequiredCommandLinePart
  )

  $stopped = @{}

  if (Test-Path -LiteralPath $PidFile) {
    $rawPid = (Get-Content -LiteralPath $PidFile -ErrorAction SilentlyContinue | Select-Object -First 1)
    $processId = 0
    if ($rawPid -and [int]::TryParse(($rawPid.ToString()).Trim(), [ref]$processId)) {
      $process = Get-CimInstance Win32_Process -Filter "ProcessId = $processId" -ErrorAction SilentlyContinue
      if ($null -ne $process -and (Test-DevDaemonProcess $process $ExecutablePath $RequiredCommandLinePart)) {
        Stop-ProcessTree -ProcessId $processId -Label "previous $Label"
        $stopped[$processId] = $true
      }
    }

    Remove-Item -LiteralPath $PidFile -Force -ErrorAction SilentlyContinue
  }

  $exeName = Split-Path -Path $ExecutablePath -Leaf
  Get-CimInstance Win32_Process -Filter "Name = '$exeName'" -ErrorAction SilentlyContinue |
    Where-Object { Test-DevDaemonProcess $_ $ExecutablePath $RequiredCommandLinePart } |
    ForEach-Object {
      $processId = [int]$_.ProcessId
      if (-not $stopped.ContainsKey($processId)) {
        Stop-ProcessTree -ProcessId $processId -Label "previous $Label"
      }
    }
}

Stop-PreviousDaemon `
  -Label "system daemon" `
  -ExecutablePath $SystemExe `
  -PidFile $SystemPidFile `
  -RequiredCommandLinePart $ConfigPath
Stop-PreviousDaemon `
  -Label "user daemon" `
  -ExecutablePath $UserExe `
  -PidFile $UserPidFile

if (-not $SkipBuild) {
  Push-Location $RepoRoot
  try {
    cargo build -p wgo-windows-daemon --bins
  } finally {
    Pop-Location
  }
}

if (-not (Test-Path $SystemExe)) {
  throw "Missing $SystemExe. Run without -SkipBuild first."
}
if (-not (Test-Path $UserExe)) {
  throw "Missing $UserExe. Run without -SkipBuild first."
}

$children = @()
$childLogs = @{}
$childPidFiles = @{}

function Show-LogTail {
  param(
    [string]$Label,
    [string]$Path,
    [int]$Tail = 80
  )

  if (-not (Test-Path $Path)) {
    Write-Host ""
    Write-Host "[$Label] log not found: $Path" -ForegroundColor Yellow
    return
  }

  Write-Host ""
  Write-Host "[$Label] last $Tail lines: $Path" -ForegroundColor Yellow
  $content = Get-Content -Path $Path -Tail $Tail -ErrorAction SilentlyContinue
  if ($content) {
    $content | ForEach-Object { Write-Host $_ }
  } else {
    Write-Host "(empty)"
  }
}

function Show-ChildLogs {
  param([System.Diagnostics.Process]$Child)

  $logs = $childLogs[$Child.Id]
  if ($null -eq $logs) {
    return
  }

  Show-LogTail "$($logs.Name) stdout" $logs.Stdout
  Show-LogTail "$($logs.Name) stderr" $logs.Stderr
}

function Stop-ChildProcesses {
  foreach ($child in $children) {
    if ($null -ne $child -and -not $child.HasExited) {
      Stop-ProcessTree -ProcessId $child.Id -Label $child.ProcessName
    }

    if ($null -ne $child) {
      $pidFile = $childPidFiles[$child.Id]
      if ($pidFile) {
        Remove-Item -LiteralPath $pidFile -Force -ErrorAction SilentlyContinue
      }
    }
  }
}

$exitSubscription = Register-EngineEvent PowerShell.Exiting -Action {
  foreach ($child in $children) {
    if ($null -ne $child -and -not $child.HasExited) {
      taskkill.exe /PID $child.Id /T /F | Out-Null
    }
  }

  foreach ($pidFile in $childPidFiles.Values) {
    if ($pidFile) {
      Remove-Item -LiteralPath $pidFile -Force -ErrorAction SilentlyContinue
    }
  }
}

try {
  Write-Host "Starting wgo Windows system daemon on $Listen"
  $system = Start-Process `
    -FilePath $SystemExe `
    -ArgumentList @("run", "--listen", $Listen, "--config", $ConfigPath) `
    -WorkingDirectory $RepoRoot `
    -PassThru `
    -RedirectStandardOutput $SystemOutLog `
    -RedirectStandardError $SystemErrLog `
    -WindowStyle Hidden
  $children += $system
  Set-Content -LiteralPath $SystemPidFile -Value $system.Id -Encoding ASCII
  $childPidFiles[$system.Id] = $SystemPidFile
  $childLogs[$system.Id] = @{
    Name = "system daemon"
    Stdout = $SystemOutLog
    Stderr = $SystemErrLog
  }

  Write-Host "Starting wgo Windows user daemon"
  $user = Start-Process `
    -FilePath $UserExe `
    -ArgumentList @("run", "--config", $ConfigPath) `
    -WorkingDirectory $RepoRoot `
    -PassThru `
    -RedirectStandardOutput $UserOutLog `
    -RedirectStandardError $UserErrLog `
    -WindowStyle Hidden
  $children += $user
  Set-Content -LiteralPath $UserPidFile -Value $user.Id -Encoding ASCII
  $childPidFiles[$user.Id] = $UserPidFile
  $childLogs[$user.Id] = @{
    Name = "user daemon"
    Stdout = $UserOutLog
    Stderr = $UserErrLog
  }

  Write-Host ""
  Write-Host "System daemon pid=$($system.Id)"
  Write-Host "User daemon pid=$($user.Id)"
  Write-Host "Dev config: $ConfigPath"
  Write-Host "Logs: $LogDir"
  Write-Host "WebTransport endpoints: https://$Listen/rpc and https://$Listen/moqt"
  Write-Host "Press Ctrl+C or close this script to stop both daemons."

  while ($true) {
    foreach ($child in $children) {
      if ($child.HasExited) {
        Show-ChildLogs $child
        throw "$($child.ProcessName) exited with code $($child.ExitCode)"
      }
    }
    Start-Sleep -Seconds 1
  }
} finally {
  Stop-ChildProcesses
  if ($exitSubscription) {
    Unregister-Event -SubscriptionId $exitSubscription.Id -ErrorAction SilentlyContinue
  }
}
