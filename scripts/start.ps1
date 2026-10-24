#Requires -Version 7.0
[CmdletBinding()]
param(
    [switch]$Release,
    [switch]$SkipInstall,
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$frontend = Join-Path $repoRoot 'frontend'
$dataDirectory = Join-Path $repoRoot '.local/dev-data'
$buildProfile = if ($Release) { 'release' } else { 'debug' }
$executable = Join-Path $repoRoot "target/$buildProfile/taskcascade-desktop.exe"

function Invoke-Native([string]$Path, [string[]]$Arguments) {
    & $Path @Arguments
    if ($LASTEXITCODE -ne 0) { throw "Command failed: $Path $($Arguments -join ' ')" }
}

# Existence checks only: never open the legacy database or any of its sidecars.
foreach ($suffix in @('', '-wal', '-shm', '-journal')) {
    if (Test-Path -LiteralPath (Join-Path $repoRoot ".local/data/taskcascade.sqlite$suffix")) {
        throw 'Your real tasks are still in .local\data. Run scripts\migrate-data.ps1 first.'
    }
}
foreach ($process in Get-Process -Name 'taskcascade-desktop' -ErrorAction SilentlyContinue) {
    if (-not $process.Path) { throw 'A taskcascade-desktop process whose executable path cannot be read is running; close that process first.' }
    if ($process.Path -eq $executable) { throw 'Please close the development window first.' }
}

if (-not $SkipInstall) {
    Push-Location $frontend
    try { Invoke-Native 'npm.cmd' @('install') } finally { Pop-Location }
}
if (-not $SkipBuild) {
    Push-Location $frontend
    try { Invoke-Native 'npm.cmd' @('run', 'build') } finally { Pop-Location }
    $arguments = @('build', '-p', 'taskcascade-desktop')
    if ($Release) { $arguments += '--release' }
    Push-Location $repoRoot
    try { Invoke-Native 'cargo' $arguments } finally { Pop-Location }
}

$previousEnvironment = @{}
foreach ($name in @('TASKCASCADE_DATA_DIR', 'TASKCASCADE_CREATE_DATABASE')) {
    $previousEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
}
try {
    New-Item -ItemType Directory -Force -Path $dataDirectory | Out-Null
    $env:TASKCASCADE_DATA_DIR = $dataDirectory
    $env:TASKCASCADE_CREATE_DATABASE = '1'
    Write-Host "Database: $(Join-Path $dataDirectory 'taskcascade.sqlite')" -ForegroundColor Cyan
    Write-Host "Log: $(Join-Path $dataDirectory 'logs/taskcascade-desktop.log')"
    Write-Host 'Close the window to stop (Ctrl+C hard-kills it without saving a pending edit)'
    if (-not $Release) {
        Write-Host 'After npm run build in frontend\, press F5 in the window to reload the interface'
    }
    $desktopProcess = Start-Process -FilePath $executable -WorkingDirectory $repoRoot -Wait -NoNewWindow -PassThru
    $exitCode = $desktopProcess.ExitCode
}
finally {
    foreach ($name in $previousEnvironment.Keys) {
        # PowerShell converts an untyped $null to an empty string here.
        $value = if ($null -eq $previousEnvironment[$name]) { [NullString]::Value } else { $previousEnvironment[$name] }
        [Environment]::SetEnvironmentVariable($name, $value, 'Process')
    }
}
exit $exitCode
