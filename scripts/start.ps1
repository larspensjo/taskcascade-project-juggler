#Requires -Version 7.0
[CmdletBinding(DefaultParameterSetName = 'Development')]
param(
    [Parameter(ParameterSetName = 'Development')]
    [switch]$Desktop,
    [Parameter(ParameterSetName = 'Development')]
    [switch]$SkipInstall,
    [Parameter(ParameterSetName = 'Development')]
    [switch]$SkipBuild,
    [Parameter(ParameterSetName = 'Development')]
    [int]$FrontendPort = 5173,
    [Parameter(ParameterSetName = 'Development')]
    [Parameter(ParameterSetName = 'RealData')]
    [int]$BackendPort = 8080,
    [Parameter(Mandatory, ParameterSetName = 'RealData')]
    [switch]$RealData,
    [Parameter(Mandatory, ParameterSetName = 'Snapshot')]
    [switch]$CreateRealDataSnapshot,
    [Parameter(ParameterSetName = 'RealData')]
    [Parameter(ParameterSetName = 'Snapshot')]
    [ValidateNotNullOrEmpty()][string]$RealDataRoot,
    [Parameter(ParameterSetName = 'RealData')]
    [Parameter(ParameterSetName = 'Snapshot')]
    [ValidateNotNullOrEmpty()][string]$SnapshotDir,
    [Parameter(ParameterSetName = 'Snapshot')]
    [switch]$Force,
    [Parameter(ParameterSetName = 'Snapshot')]
    [switch]$AllowDirtyForRehearsal
)

$ErrorActionPreference = "Stop"
if ($Desktop -and ($PSBoundParameters.ContainsKey('FrontendPort') -or $PSBoundParameters.ContainsKey('BackendPort'))) {
    throw '-Desktop cannot be combined with -FrontendPort or -BackendPort; desktop mode does not use TCP ports.'
}
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$backend = Join-Path $repoRoot "backend"
$frontend = Join-Path $repoRoot "frontend"
$dataDirectory = Join-Path $repoRoot ".local/dev-data"
$defaultSnapshotDir = Join-Path $repoRoot '.local/real-run'
$defaultRealDataRoot = if ($env:LOCALAPPDATA) { Join-Path $env:LOCALAPPDATA 'TaskCascade' }
if ($RealData -or $CreateRealDataSnapshot) {
    if (-not $RealDataRoot) {
        if (-not $env:LOCALAPPDATA) { throw 'LOCALAPPDATA is unset; provide -RealDataRoot.' }
        $RealDataRoot = Join-Path $env:LOCALAPPDATA 'TaskCascade'
    }
    $RealDataRoot = $PSCmdlet.GetUnresolvedProviderPathFromPSPath($RealDataRoot).TrimEnd('\', '/')
    if (-not $SnapshotDir) { $SnapshotDir = $defaultSnapshotDir }
    $SnapshotDir = $PSCmdlet.GetUnresolvedProviderPathFromPSPath($SnapshotDir).TrimEnd('\', '/')
}
if ($AllowDirtyForRehearsal -and $SnapshotDir -eq $defaultSnapshotDir) {
    throw '-AllowDirtyForRehearsal requires a non-default -SnapshotDir.'
}

function Invoke-Native([string]$Path, [string[]]$Arguments) {
    & $Path @Arguments
    if ($LASTEXITCODE -ne 0) { throw "Command failed: $Path $($Arguments -join ' ')" }
}

function Stop-ProcessTree([System.Diagnostics.Process]$Process) {
    if ($Process -and -not $Process.HasExited) {
        $Process.Kill($true)
        if (-not $Process.WaitForExit(5000)) { throw "Process tree $($Process.Id) did not stop." }
    }
}

function Wait-Url([string]$Url, [System.Diagnostics.Process]$Process) {
    $deadline = (Get-Date).AddSeconds(20)
    while ((Get-Date) -lt $deadline) {
        if ($Process.HasExited) { throw "The process for $Url exited with code $($Process.ExitCode)." }
        try {
            Invoke-WebRequest -Uri $Url -UseBasicParsing -TimeoutSec 1 | Out-Null
            return
        }
        catch { Start-Sleep -Milliseconds 250 }
    }
    throw "Timed out waiting for $Url."
}

function Test-PortAvailable([int]$Port) {
    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $Port)
    try {
        $listener.Start()
        return $true
    }
    catch {
        return $false
    }
    finally {
        $listener.Stop()
    }
}

function Resolve-AvailablePort([int]$PreferredPort, [int[]]$ExcludedPorts = @()) {
    for ($port = $PreferredPort; $port -le 65535; $port++) {
        if ($port -notin $ExcludedPorts -and (Test-PortAvailable $port)) {
            return $port
        }
    }
    throw "No free loopback port was found starting at $PreferredPort."
}

function Assert-SnapshotStopped {
    foreach ($process in Get-Process -Name 'taskcascade-backend' -ErrorAction SilentlyContinue) {
        $path = $process.Path
        if (-not $path) { throw 'Cannot determine a running backend path; close it before using the snapshot.' }
        if ($path -eq (Join-Path $SnapshotDir 'backend/taskcascade-backend.exe')) {
            throw 'A snapshot executable is already running. Stop it first.'
        }
    }
}

function Get-SnapshotFiles {
    Get-ChildItem -LiteralPath (Join-Path $SnapshotDir 'backend'), (Join-Path $SnapshotDir 'frontend/dist') -File -Recurse -Force
}

function Assert-SnapshotValid {
    $changed = 'The snapshot changed since it was created - recreate it explicitly with -CreateRealDataSnapshot -Force'
    try {
        $manifest = Get-Content -LiteralPath (Join-Path $SnapshotDir 'snapshot.json') -Raw | ConvertFrom-Json
        if (-not $manifest.revision -or -not $manifest.createdUtc -or -not $manifest.files) { throw $changed }
        $listed = @()
        foreach ($file in $manifest.files) {
            $path = [IO.Path]::GetFullPath((Join-Path $SnapshotDir $file.path))
            if (-not $path.StartsWith("$SnapshotDir\", [StringComparison]::OrdinalIgnoreCase)) { throw $changed }
            if ($file.path -in $listed -or -not (Test-Path -LiteralPath $path -PathType Leaf)) { throw $changed }
            if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $file.sha256) { throw $changed }
            $listed += $file.path
        }
        foreach ($file in Get-SnapshotFiles) {
            $relative = [IO.Path]::GetRelativePath($SnapshotDir, $file.FullName).Replace('\', '/')
            if ($relative -notin $listed) { throw $changed }
        }
        if ('backend/taskcascade-backend.exe' -notin $listed -or 'frontend/dist/index.html' -notin $listed) { throw $changed }
        return $manifest
    }
    catch { throw "$changed. $($_.Exception.Message)" }
}

$previousEnvironment = @{}
foreach ($name in @('TASKCASCADE_DATA_DIR', 'TASKCASCADE_PORT')) {
    $previousEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
}
$backendProcess = $null
$frontendProcess = $null

try {
    if ($CreateRealDataSnapshot) {
        # Never let a snapshot replacement delete a source tree or a data root.
        foreach ($protected in @($repoRoot, $backend, $frontend, $RealDataRoot)) {
            $protected = [IO.Path]::GetFullPath($protected).TrimEnd('\', '/')
            if ($protected -eq $SnapshotDir -or $protected.StartsWith("$SnapshotDir\", [StringComparison]::OrdinalIgnoreCase) -or
                $SnapshotDir.StartsWith("$RealDataRoot\", [StringComparison]::OrdinalIgnoreCase)) { throw 'Snapshot directory overlaps a source or data directory.' }
        }
        foreach ($sourceTree in @($backend, $frontend)) {
            if ($SnapshotDir.StartsWith("$sourceTree\", [StringComparison]::OrdinalIgnoreCase)) { throw 'Snapshot directory overlaps a source tree.' }
        }
        Assert-SnapshotStopped
        Push-Location $repoRoot
        try {
            $status = & git status --porcelain
            if ($LASTEXITCODE -ne 0) { throw 'Could not inspect working tree.' }
            if ($status -and -not $AllowDirtyForRehearsal) { throw 'Snapshot creation requires a clean working tree.' }
            $revision = & git rev-parse HEAD
            if ($LASTEXITCODE -ne 0) { throw 'Could not determine git revision.' }
        }
        finally { Pop-Location }
        $existing = Test-Path -LiteralPath (Join-Path $SnapshotDir 'snapshot.json')
        if ($existing -and -not $Force) { throw 'Snapshot already exists; use -CreateRealDataSnapshot -Force to replace it explicitly.' }
        if ((Test-Path -LiteralPath $SnapshotDir) -and -not $existing -and @(Get-ChildItem -LiteralPath $SnapshotDir -Force).Count) {
            throw 'Snapshot directory contains unrelated files; use an empty directory.'
        }
        Write-Host "Creating snapshot - data root $RealDataRoot - snapshot $SnapshotDir"
        Push-Location $frontend
        try { Invoke-Native 'npm.cmd' @('ci'); Invoke-Native 'npm.cmd' @('run', 'build') } finally { Pop-Location }
        Push-Location $repoRoot
        try { Invoke-Native 'cargo' @('build', '-p', 'taskcascade-backend') } finally { Pop-Location }
        Assert-SnapshotStopped
        New-Item -ItemType Directory -Force -Path $SnapshotDir | Out-Null
        # Only these managed subdirectories are replaced, after successful builds.
        foreach ($relative in @('backend', 'frontend')) {
            $managed = [IO.Path]::GetFullPath((Join-Path $SnapshotDir $relative))
            if ((Split-Path -Parent $managed) -ne $SnapshotDir) { throw 'Snapshot cleanup escaped its directory.' }
            if (Test-Path -LiteralPath $managed) { Remove-Item -LiteralPath $managed -Recurse -Force }
        }
        New-Item -ItemType Directory -Path (Join-Path $SnapshotDir 'backend'), (Join-Path $SnapshotDir 'frontend/dist') | Out-Null
        Copy-Item -LiteralPath (Join-Path $repoRoot 'target/debug/taskcascade-backend.exe') -Destination (Join-Path $SnapshotDir 'backend/taskcascade-backend.exe')
        Get-ChildItem -LiteralPath (Join-Path $frontend 'dist') -Force | Copy-Item -Destination (Join-Path $SnapshotDir 'frontend/dist') -Recurse
        $files = @(Get-SnapshotFiles | ForEach-Object {
            [ordered]@{ path = [IO.Path]::GetRelativePath($SnapshotDir, $_.FullName).Replace('\', '/'); sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash }
        })
        [ordered]@{ revision = $revision; createdUtc = [DateTime]::UtcNow.ToString('o'); rehearsalOnly = [bool]$AllowDirtyForRehearsal; files = $files } |
            ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $SnapshotDir 'snapshot.json') -Encoding utf8
        $null = Assert-SnapshotValid
        Write-Host "Created snapshot $revision at $SnapshotDir"
        return
    }
    if ($RealData) {
        $manifest = Assert-SnapshotValid
        if ($manifest.rehearsalOnly -and $defaultRealDataRoot -and
            $RealDataRoot -eq $PSCmdlet.GetUnresolvedProviderPathFromPSPath($defaultRealDataRoot).TrimEnd('\', '/')) {
            throw 'A rehearsal-only snapshot cannot run against the default real-data root. Create a clean snapshot first.'
        }
        if (-not (Test-Path -LiteralPath (Join-Path $RealDataRoot 'taskcascade.sqlite') -PathType Leaf)) {
            throw "Real database is missing at $RealDataRoot; run scripts\migrate-data.ps1 first."
        }
        Assert-SnapshotStopped
        $dataDirectory = $RealDataRoot
        $resolvedBackendPort = Resolve-AvailablePort $BackendPort
        $env:TASKCASCADE_DATA_DIR = $dataDirectory
        $env:TASKCASCADE_PORT = $resolvedBackendPort
        $backendProcess = Start-Process -FilePath (Join-Path $SnapshotDir 'backend/taskcascade-backend.exe') -WorkingDirectory (Join-Path $SnapshotDir 'backend') -PassThru -WindowStyle Hidden
        Wait-Url "http://127.0.0.1:$resolvedBackendPort/api/health" $backendProcess
        Write-Host "REAL DATA - $RealDataRoot - snapshot $($manifest.revision)"
        $url = "http://127.0.0.1:$resolvedBackendPort/"
        Write-Host "TaskCascade is running at $url (Ctrl+C to stop)."
        Start-Process $url
        while (-not $backendProcess.HasExited) { Start-Sleep -Milliseconds 400 }
        throw "Backend exited with code $($backendProcess.ExitCode)."
    }
    # Only default development mode guards legacy data. Snapshot creation never
    # touches data, and -RealData requires an already-migrated destination.
    foreach ($suffix in @('', '-wal', '-shm', '-journal')) {
        if (Test-Path -LiteralPath (Join-Path $repoRoot ".local/data/taskcascade.sqlite$suffix")) {
            throw 'Your real tasks are still in .local\data. Run scripts\migrate-data.ps1 first.'
        }
    }
    if (-not $SkipInstall) {
        Push-Location $frontend
        try { Invoke-Native 'npm.cmd' @('install') } finally { Pop-Location }
    }
    if (-not $SkipBuild) {
        Push-Location $frontend
        try { Invoke-Native 'npm.cmd' @('run', 'build') } finally { Pop-Location }
        $package = if ($Desktop) { 'taskcascade-desktop' } else { 'taskcascade-backend' }
        Push-Location $repoRoot
        try { Invoke-Native 'cargo' @('build', '-p', $package) } finally { Pop-Location }
    }
    New-Item -ItemType Directory -Force -Path $dataDirectory | Out-Null
    if ($Desktop) {
        $env:TASKCASCADE_DATA_DIR = $dataDirectory
        Write-Host "DEVELOPMENT - $dataDirectory" -ForegroundColor Cyan
        Write-Host 'Close the window to stop. Ctrl+C here hard-kills the window without saving a pending edit.'
        $desktopProcess = Start-Process -FilePath (Join-Path $repoRoot 'target/debug/taskcascade-desktop.exe') -WorkingDirectory $repoRoot -Wait -NoNewWindow -PassThru
        if ($desktopProcess.ExitCode -ne 0) { throw "Desktop exited with code $($desktopProcess.ExitCode)." }
        return
    }
    $resolvedFrontendPort = Resolve-AvailablePort $FrontendPort
    $resolvedBackendPort = Resolve-AvailablePort $BackendPort @($resolvedFrontendPort)
    $env:TASKCASCADE_DATA_DIR = $dataDirectory
    $env:TASKCASCADE_PORT = $resolvedBackendPort
    $backendProcess = Start-Process -FilePath (Join-Path $repoRoot 'target/debug/taskcascade-backend.exe') -WorkingDirectory $backend -PassThru -WindowStyle Hidden
    # cmd.exe stays alive while Vite runs, so process-tree cleanup owns both.
    $frontendCommand = "call npm.cmd run dev -- --host 127.0.0.1 --port $resolvedFrontendPort --strictPort"
    $frontendProcess = Start-Process -FilePath 'cmd.exe' -ArgumentList @('/d', '/s', '/c', $frontendCommand) -WorkingDirectory $frontend -PassThru -WindowStyle Hidden
    Wait-Url "http://127.0.0.1:$resolvedBackendPort/api/health" $backendProcess
    Wait-Url "http://127.0.0.1:$resolvedFrontendPort" $frontendProcess
    if ($resolvedFrontendPort -ne $FrontendPort) {
        Write-Host "Frontend port $FrontendPort was busy; using $resolvedFrontendPort." -ForegroundColor Yellow
    }
    if ($resolvedBackendPort -ne $BackendPort) {
        Write-Host "Backend port $BackendPort was busy; using $resolvedBackendPort." -ForegroundColor Yellow
    }
    Write-Host "DEVELOPMENT - $dataDirectory - http://127.0.0.1:$resolvedFrontendPort (Ctrl+C to stop)." -ForegroundColor Cyan

    while ($true) {
        if ($backendProcess.HasExited) { throw "Backend exited with code $($backendProcess.ExitCode)." }
        if ($frontendProcess.HasExited) { throw "Frontend exited with code $($frontendProcess.ExitCode)." }
        Start-Sleep -Milliseconds 400
    }
}
finally {
    try {
        $stopErrors = @()
        foreach ($process in @($frontendProcess, $backendProcess)) {
            try { Stop-ProcessTree $process }
            catch { $stopErrors += $_.Exception.Message }
        }
        if ($stopErrors.Count) { Write-Warning "Process cleanup failures: $($stopErrors -join ' ')" }
    }
    finally {
        foreach ($name in $previousEnvironment.Keys) {
            # PowerShell converts an untyped $null to an empty string here.
            $value = if ($null -eq $previousEnvironment[$name]) { [NullString]::Value } else { $previousEnvironment[$name] }
            [Environment]::SetEnvironmentVariable($name, $value, 'Process')
        }
    }
}
