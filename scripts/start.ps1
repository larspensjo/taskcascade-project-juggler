[CmdletBinding()]
param(
    [switch]$SkipInstall,
    [switch]$SkipBuild,
    [int]$FrontendPort = 5173,
    [int]$BackendPort = 8080
)

$ErrorActionPreference = "Stop"
$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$backend = Join-Path $repoRoot "backend"
$frontend = Join-Path $repoRoot "frontend"
$dataDirectory = Join-Path $repoRoot ".local/data"

function Invoke-Native([string]$Path, [string[]]$Arguments) {
    & $Path @Arguments
    if ($LASTEXITCODE -ne 0) { throw "Command failed: $Path $($Arguments -join ' ')" }
}

function Stop-ProcessTree([System.Diagnostics.Process]$Process) {
    if ($Process -and -not $Process.HasExited) {
        & taskkill.exe /PID $Process.Id /T /F | Out-Null
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

if (-not $SkipInstall) {
    Push-Location $frontend
    try { Invoke-Native "npm.cmd" @("install") } finally { Pop-Location }
}
if (-not $SkipBuild) {
    Push-Location $backend
    try { Invoke-Native "cargo" @("build") } finally { Pop-Location }
    Push-Location $frontend
    try { Invoke-Native "npm.cmd" @("run", "build") } finally { Pop-Location }
}

New-Item -ItemType Directory -Force -Path $dataDirectory | Out-Null
$env:TASKCASCADE_DATA_DIR = $dataDirectory
$env:TASKCASCADE_PORT = $BackendPort
$backendProcess = Start-Process -FilePath (Join-Path $backend "target/debug/taskcascade-backend.exe") -WorkingDirectory $backend -PassThru -WindowStyle Hidden
# Start npm through cmd.exe + call. Start-Process otherwise returns the short-lived
# npm.cmd wrapper, which ends while Vite's node process is still running.
$frontendCommand = "call npm.cmd run dev -- --host 127.0.0.1 --port $FrontendPort --strictPort"
$frontendProcess = Start-Process -FilePath "cmd.exe" -ArgumentList @("/d", "/s", "/c", $frontendCommand) -WorkingDirectory $frontend -PassThru -WindowStyle Hidden

try {
    Wait-Url "http://127.0.0.1:$BackendPort/api/health" $backendProcess
    Wait-Url "http://127.0.0.1:$FrontendPort" $frontendProcess
    Write-Host "TaskCascade is running at http://127.0.0.1:$FrontendPort (Ctrl+C to stop)." -ForegroundColor Cyan

    while ($true) {
        if ($backendProcess.HasExited) { throw "Backend exited with code $($backendProcess.ExitCode)." }
        if ($frontendProcess.HasExited) { throw "Frontend exited with code $($frontendProcess.ExitCode)." }
        Start-Sleep -Milliseconds 400
    }
}
finally {
    Stop-ProcessTree $frontendProcess
    Stop-ProcessTree $backendProcess
}
