#Requires -Version 7.0
[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateNotNullOrEmpty()][string]$Root,
    [switch]$AllowDirtyForRehearsal
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'rehearsal-support.ps1')
$ownedRoot = Initialize-RehearsalRoot $Root
$launcher = Join-Path $repoRoot 'scripts/start.ps1'
$data = Join-Path $ownedRoot 'data'
$snapshot = Join-Path $ownedRoot 'snap'
$extraName = "snapshot-rehearsal-$([guid]::NewGuid()).txt"
$extraPath = Join-Path $repoRoot "frontend/dist/$extraName"
$state = @{}
$previousEnvironment = @{}
foreach ($name in @('TASKCASCADE_DATA_DIR', 'TASKCASCADE_PORT')) { $previousEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }

function Stop-RehearsalLauncher {
    if ($state.job) {
        # Stop only this rehearsal's executable, then dispose its launcher job.
        # taskkill /T can fail under a restricted token and leave SQLite locked.
        $expectedExecutable = Join-Path $snapshot 'backend/taskcascade-backend.exe'
        foreach ($process in Get-Process -Name 'taskcascade-backend' -ErrorAction SilentlyContinue) {
            if ($process.Path -eq $expectedExecutable) {
                $process.Kill($true)
                Assert-That ($process.WaitForExit(5000)) 'Snapshot backend did not stop.'
            }
        }
        Stop-Job $state.job
        Receive-Job $state.job -ErrorAction Continue | Out-Host
        Remove-Job $state.job
        $state.job = $null
    }
}

try {
    Invoke-Case 'cleanup attempts both process trees and reports both failures' {
        $ast = [Management.Automation.Language.Parser]::ParseFile($launcher, [ref]$null, [ref]$null)
        $launcherTry = $ast.EndBlock.Statements | Where-Object { $_ -is [Management.Automation.Language.TryStatementAst] } | Select-Object -Last 1
        $cleanup = [scriptblock]::Create(($launcherTry.Finally.Statements.Extent.Text -join "`n"))
        $attempts = [Collections.Generic.List[string]]::new()
        function Stop-ProcessTree($Process) { $attempts.Add($Process); throw "simulated $Process stop failure" }
        $frontendProcess = 'frontend'
        $backendProcess = 'backend'
        $previousEnvironment = @{}
        $warnings = @(& $cleanup 3>&1)
        Assert-That (($attempts -join ',') -eq 'frontend,backend') 'Cleanup did not attempt both process trees.'
        Assert-That ($warnings.Count -eq 1 -and $warnings[0] -like '*simulated frontend stop failure*simulated backend stop failure*') 'Cleanup did not report both failures together.'
    }
    Invoke-Case '1 generate plain fixture' { New-Fixture $data 'plain' }
    Invoke-Case '2 create fixed snapshot' {
        # Force is used only in the fresh, validated rehearsal root.
        # Relative caller paths must follow PowerShell's location, which can
        # differ from the process working directory.
        Push-Location $ownedRoot
        try { & $launcher -CreateRealDataSnapshot -RealDataRoot './data' -SnapshotDir './snap' -Force -AllowDirtyForRehearsal:$AllowDirtyForRehearsal }
        finally { Pop-Location }
        foreach ($name in $previousEnvironment.Keys) {
            Assert-That ([Environment]::GetEnvironmentVariable($name, 'Process') -ceq $previousEnvironment[$name]) "Snapshot creation changed $name."
        }
        $manifest = Get-Content -LiteralPath (Join-Path $snapshot 'snapshot.json') -Raw | ConvertFrom-Json
        Assert-That ($manifest.rehearsalOnly -ceq [bool]$AllowDirtyForRehearsal) 'Snapshot rehearsal marker differs from the build mode.'
    }
    Invoke-Case 'rehearsal snapshot refuses the default root using synthetic LOCALAPPDATA' {
        $manifestPath = Join-Path $snapshot 'snapshot.json'
        $original = Get-Content -LiteralPath $manifestPath -Raw
        $fakeAppData = Join-Path $ownedRoot 'localappdata'
        $fakeDefault = Join-Path $fakeAppData 'TaskCascade'
        try {
            $manifest = $original | ConvertFrom-Json
            $manifest.rehearsalOnly = $true
            $manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $manifestPath -Encoding utf8
            $result = Invoke-RehearsalPowerShell $launcher @('-RealData', '-RealDataRoot', $fakeDefault, '-SnapshotDir', $snapshot) @{ LOCALAPPDATA = $fakeAppData }
            Assert-That ($result.ExitCode -ne 0 -and $result.Output -like '*rehearsal-only snapshot cannot run against the default real-data root*') "Expected rehearsal snapshot refusal, got $($result.Output)"
            Assert-That (-not (Test-Path -LiteralPath $fakeDefault)) 'Rehearsal snapshot refusal created a data directory.'
        }
        finally { Set-Content -LiteralPath $manifestPath -Value $original -NoNewline -Encoding utf8 }
    }
    Invoke-Case '3 record snapshot hashes, times and manifest' {
        $state.before = Get-TreeRecord $snapshot
        $state.manifest = Get-Content -LiteralPath (Join-Path $snapshot 'snapshot.json') -Raw
    }
    Invoke-Case '4 rebuild repository artifacts and add an extra dist file' {
        Push-Location (Join-Path $repoRoot 'frontend')
        try {
            & npm.cmd run build
            if ($LASTEXITCODE -ne 0) { throw 'npm run build failed.' }
        }
        finally { Pop-Location }
        Invoke-Backend @('build') | Out-Null
        [IO.File]::WriteAllText($extraPath, 'This repository file must never enter the fixed snapshot.')
    }
    Invoke-Case '5 launch with cargo and npm absent from PATH' {
        $toolDirectories = @('cargo', 'npm.cmd', 'node.exe' | ForEach-Object {
            Get-Command $_ -All -ErrorAction SilentlyContinue | ForEach-Object { Split-Path -Parent $_.Source }
        })
        $strippedPath = ($env:PATH.Split(';') | Where-Object { $_.TrimEnd('\', '/') -notin $toolDirectories }) -join ';'
        $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
        try { $listener.Start(); $state.port = $listener.LocalEndpoint.Port } finally { $listener.Stop() }
        $state.job = Start-Job -ArgumentList $launcher, $data, $snapshot, $strippedPath, $state.port -ScriptBlock {
            param($Launcher, $Data, $Snapshot, $StrippedPath, $Port)
            $ErrorActionPreference = 'Stop'
            $env:PATH = $StrippedPath
            if ((Get-Command cargo -ErrorAction SilentlyContinue) -or (Get-Command npm.cmd -ErrorAction SilentlyContinue)) { throw 'PATH still provides cargo or npm.' }
            & $Launcher -RealData -RealDataRoot $Data -SnapshotDir $Snapshot -BackendPort $Port
        }
    }
    try {
        Invoke-Case '6 bootstrap answers with fixture tasks, then stop the process tree' {
            $deadline = [DateTime]::UtcNow.AddSeconds(30)
            $bootstrap = $null
            while ([DateTime]::UtcNow -lt $deadline) {
                if ($state.job.State -in @('Completed', 'Failed', 'Stopped')) {
                    Receive-Job $state.job -ErrorAction Continue | Out-Host
                    throw 'Snapshot launcher exited before bootstrap answered.'
                }
                try { $bootstrap = Invoke-RestMethod "http://127.0.0.1:$($state.port)/api/bootstrap" -TimeoutSec 1; break }
                catch { Start-Sleep -Milliseconds 200 }
            }
            Assert-That ($null -ne $bootstrap) 'Timed out waiting for bootstrap.'
            $expected = Get-Content -LiteralPath (Join-Path $data 'expected.json') -Raw | ConvertFrom-Json
            Assert-That ($bootstrap.projects.Count -eq $expected.projects -and $bootstrap.activeTasks.Count -eq $expected.activeTasks -and
                $bootstrap.archivedTasks.Count -eq $expected.archivedTasks -and $bootstrap.deletedTasks.Count -eq $expected.deletedTasks -and
                $bootstrap.preferences.Count -eq $expected.preferences) 'Bootstrap counts differ from fixture.'
            $titles = @($bootstrap.activeTasks.title) + @($bootstrap.archivedTasks.title) + @($bootstrap.deletedTasks.title)
            Assert-That (($titles | Sort-Object) -join ',' -eq 'active-1,active-2,archived,deleted') 'Bootstrap tasks differ from fixture.'
            $html = Invoke-WebRequest "http://127.0.0.1:$($state.port)/" -TimeoutSec 2
            Assert-That ($html.StatusCode -eq 200 -and $html.Content -match '<!doctype html>') 'Snapshot frontend was not served.'
            # Do not accept a transient API response if opening the browser then fails.
            Start-Sleep -Milliseconds 500
            if ($state.job.State -ne 'Running') {
                Receive-Job $state.job -ErrorAction Continue | Out-Host
                throw 'Snapshot launcher failed during startup (see job error above).'
            }
            Assert-Refused { & $launcher -RealData -RealDataRoot $data -SnapshotDir $snapshot } 'snapshot executable is already running'
            Stop-RehearsalLauncher
        }
    }
    catch {
        $state.launchFailure = $_
        Stop-RehearsalLauncher
    }
    Invoke-Case '7 snapshot is unchanged and excludes the extra dist file' {
        Assert-That ((Get-TreeRecord $snapshot) -eq $state.before) 'Snapshot files, hashes or write times changed.'
        Assert-That ((Get-Content -LiteralPath (Join-Path $snapshot 'snapshot.json') -Raw) -ceq $state.manifest) 'Manifest changed.'
        Assert-That (-not (Test-Path -LiteralPath (Join-Path $snapshot "frontend/dist/$extraName"))) 'Repository file was copied into snapshot.'
    }
    Invoke-Case '8 tampered snapshot is refused' {
        $unlisted = Join-Path $snapshot 'frontend/dist/unlisted.txt'
        [IO.File]::WriteAllText($unlisted, 'unlisted')
        Assert-Refused { & $launcher -RealData -RealDataRoot $data -SnapshotDir $snapshot } 'snapshot changed since it was created'
        Remove-Item -LiteralPath $unlisted
        Add-Content -LiteralPath (Join-Path $snapshot 'frontend/dist/index.html') -Value 'tampered'
        Assert-Refused { & $launcher -RealData -RealDataRoot $data -SnapshotDir $snapshot } 'snapshot changed since it was created'
        foreach ($name in $previousEnvironment.Keys) {
            Assert-That ([Environment]::GetEnvironmentVariable($name, 'Process') -ceq $previousEnvironment[$name]) "Refusal changed $name."
        }
        $env:TASKCASCADE_DATA_DIR = $data
        $env:TASKCASCADE_PORT = '12345'
        Assert-Refused { & $launcher -RealData -RealDataRoot $data -SnapshotDir $snapshot } 'snapshot changed since it was created'
        Assert-That ($env:TASKCASCADE_DATA_DIR -ceq $data -and $env:TASKCASCADE_PORT -ceq '12345') 'Refusal failed to restore existing environment values.'
    }
    if ($state.launchFailure) { throw $state.launchFailure }
}
finally {
    try { Stop-RehearsalLauncher }
    finally {
        if (Test-Path -LiteralPath $extraPath) { Remove-Item -LiteralPath $extraPath -Force }
        foreach ($name in $previousEnvironment.Keys) {
            $value = if ($null -eq $previousEnvironment[$name]) { [NullString]::Value } else { $previousEnvironment[$name] }
            [Environment]::SetEnvironmentVariable($name, $value, 'Process')
        }
        Remove-RehearsalRoot $ownedRoot
    }
}
