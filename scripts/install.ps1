#Requires -Version 7.0
[CmdletBinding()]
param(
    [ValidateNotNullOrEmpty()][string]$InstallDir,
    [ValidateNotNullOrEmpty()][string]$ShortcutPath,
    [switch]$NoShortcut,
    [ValidateNotNullOrEmpty()][string]$DataDir,
    [switch]$SkipBuild,
    [switch]$SkipInstall,
    [switch]$InitDatabase
)

$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
if ([string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) { throw 'LOCALAPPDATA is unset; cannot resolve TaskCascade installation and data defaults.' }
if (-not $InstallDir) { $InstallDir = Join-Path $env:LOCALAPPDATA 'Programs/TaskCascade' }
if (-not $ShortcutPath) { $ShortcutPath = Join-Path ([Environment]::GetFolderPath('Programs')) 'TaskCascade.lnk' }
# The defining data-location default is desktop/src/data_location.rs.
$defaultDataDir = Join-Path $env:LOCALAPPDATA 'TaskCascade'
$defaultDataDir = $PSCmdlet.GetUnresolvedProviderPathFromPSPath($defaultDataDir).TrimEnd('\', '/')
if (-not $DataDir) { $DataDir = $defaultDataDir }
$InstallDir = $PSCmdlet.GetUnresolvedProviderPathFromPSPath($InstallDir).TrimEnd('\', '/')
$ShortcutPath = $PSCmdlet.GetUnresolvedProviderPathFromPSPath($ShortcutPath)
$DataDir = $PSCmdlet.GetUnresolvedProviderPathFromPSPath($DataDir).TrimEnd('\', '/')
$executable = Join-Path $InstallDir 'taskcascade-desktop.exe'
$database = Join-Path $DataDir 'taskcascade.sqlite'

function Invoke-Native([string]$Path, [string[]]$Arguments) {
    & $Path @Arguments
    if ($LASTEXITCODE -ne 0) { throw "Command failed: $Path $($Arguments -join ' ')" }
}

function Assert-InstalledCopyStopped {
    foreach ($process in Get-Process -Name 'taskcascade-desktop' -ErrorAction SilentlyContinue) {
        if (-not $process.Path) { throw 'Cannot determine a running TaskCascade executable path. Close TaskCascade first.' }
        if ($process.Path -eq $executable) { throw 'Close TaskCascade first, then re-run the installer.' }
    }
}

# Existence checks only: never open the legacy database or any of its sidecars.
foreach ($suffix in @('', '-wal', '-shm', '-journal')) {
    if (Test-Path -LiteralPath (Join-Path $repoRoot ".local/data/taskcascade.sqlite$suffix")) {
        throw 'Legacy .local/data/taskcascade.sqlite* exists. Run scripts/migrate-data.ps1 before installing.'
    }
}
Assert-InstalledCopyStopped
if (-not $SkipBuild) {
    Push-Location (Join-Path $repoRoot 'frontend')
    try {
        if (-not $SkipInstall) { Invoke-Native 'npm.cmd' @('install') }
        Invoke-Native 'npm.cmd' @('run', 'build')
    }
    finally { Pop-Location }
    Push-Location $repoRoot
    try { Invoke-Native 'cargo' @('build', '-p', 'taskcascade-desktop', '--release') }
    finally { Pop-Location }
}

$source = Join-Path $repoRoot 'target/release/taskcascade-desktop.exe'
if (-not (Test-Path -LiteralPath $source -PathType Leaf)) { throw "Release executable missing: $source. Run again without -SkipBuild." }
Push-Location $repoRoot
try {
    $revision = & git rev-parse HEAD
    if ($LASTEXITCODE -ne 0) { throw 'Could not determine git revision.' }
    $status = & git status --porcelain
    if ($LASTEXITCODE -ne 0) { throw 'Could not inspect working tree.' }
}
finally { Pop-Location }
Assert-InstalledCopyStopped
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
$temporary = Join-Path $InstallDir "taskcascade-desktop.$([guid]::NewGuid()).tmp"
try {
    Copy-Item -LiteralPath $source -Destination $temporary
    # Same-directory rename replaces the executable only after copying succeeds.
    try { [IO.File]::Move($temporary, $executable, $true) }
    catch { throw "Could not replace $executable. Close TaskCascade first, then re-run the installer. $($_.Exception.Message)" }
}
finally { if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary -Force } }
$installInfo = "git HEAD: $revision`ndirty: $([bool]$status)`ninstalled UTC: $([DateTime]::UtcNow.ToString('o'))"
$infoPath = Join-Path $InstallDir 'install-info.txt'
Set-Content -LiteralPath $infoPath -Value $installInfo -Encoding utf8

if (-not $NoShortcut) {
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $ShortcutPath) | Out-Null
    $shell = New-Object -ComObject WScript.Shell
    $shortcut = $shell.CreateShortcut($ShortcutPath)
    try {
        $shortcut.TargetPath = $executable
        $shortcut.Arguments = ''
        $shortcut.IconLocation = "$executable,0"
        $shortcut.WorkingDirectory = $InstallDir
        $shortcut.Save()
    }
    finally {
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($shortcut)
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($shell)
    }
}

if (Test-Path -LiteralPath $database -PathType Leaf) {
    if ($InitDatabase) { Write-Host "Database already exists; -InitDatabase did nothing: $database" }
}
elseif ($InitDatabase) {
    $previousEnvironment = @{}
    foreach ($name in @('TASKCASCADE_CREATE_DATABASE', 'TASKCASCADE_DATA_DIR')) {
        $previousEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
    }
    $child = $null
    try {
        try {
            $env:TASKCASCADE_CREATE_DATABASE = '1'
            # Clear an inherited override for the default, rather than creating
            # a different database from the one the summary reports.
            $env:TASKCASCADE_DATA_DIR = if ($DataDir -ne $defaultDataDir) { $DataDir } else { $null }
            $child = Start-Process -FilePath $executable -WorkingDirectory $InstallDir -PassThru
        }
        finally {
            foreach ($name in $previousEnvironment.Keys) {
                $value = if ($null -eq $previousEnvironment[$name]) { [NullString]::Value } else { $previousEnvironment[$name] }
                [Environment]::SetEnvironmentVariable($name, $value, 'Process')
            }
        }
        $logPath = Join-Path $DataDir 'logs/taskcascade-desktop.log'
        $ready = $false
        $deadline = [DateTime]::UtcNow.AddSeconds(30)
        while ([DateTime]::UtcNow -lt $deadline) {
            $child.Refresh()
            if ($child.HasExited) { throw 'Database initialization exited early; another window may own the single-instance identity. Close that TaskCascade window and retry.' }
            if ($child.MainWindowTitle -eq 'TaskCascade failure') { throw "TaskCascade reported a startup failure. See $(Join-Path $DataDir 'logs/taskcascade-desktop.log')." }
            $evalOk = (Test-Path -LiteralPath $logPath) -and
                (Get-Content -LiteralPath $logPath -Raw) -match "(?m)pid=$($child.Id) eval self-test ok\r?$"
            if ((Test-Path -LiteralPath $database -PathType Leaf) -and $child.MainWindowHandle -ne 0 -and
                $child.MainWindowTitle -in @('TaskCascade', 'TaskCascade (dev)') -and $evalOk) {
                $ready = $true
                break
            }
            Start-Sleep -Milliseconds 100
        }
        if (-not $ready) { throw "Timed out initializing database: $database" }
    }
    finally {
        if ($child) {
            try {
                if (-not $child.HasExited) {
                    $closing = $child.CloseMainWindow()
                    if (-not $closing -or -not $child.WaitForExit(12000)) {
                        Write-Warning 'Database initialization child did not close gracefully; forcing it to stop.'
                        $child.Kill($true)
                        if (-not $child.WaitForExit(5000)) { throw 'Database initialization child did not stop.' }
                    }
                }
            }
            finally { $child.Dispose() }
        }
    }
}
else {
    Write-Host "TaskCascade will refuse to start: no database at $database. Re-run this script with -InitDatabase to create one, or restore the migrated database."
}

Write-Host "Installed executable: $executable"
Write-Host "Install info ($infoPath):`n$installInfo"
Write-Host "Shortcut: $(if ($NoShortcut) { 'none' } else { $ShortcutPath })"
Write-Host "Database: $database (exists: $(Test-Path -LiteralPath $database -PathType Leaf))"
Write-Host 'Re-run this script to update; rebuilding in the repository never changes the installed copy.'
Write-Host 'To uninstall, delete the install directory and shortcut; the data stays.'
