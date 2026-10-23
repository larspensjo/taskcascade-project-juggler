#Requires -Version 7.0
[CmdletBinding()]
param([Parameter(Mandatory)][ValidateNotNullOrEmpty()][string]$Root)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'rehearsal-support.ps1')
$ownedRoot = Initialize-RehearsalRoot $Root
$installer = Join-Path $repoRoot 'scripts/install.ps1'
$install = Join-Path $ownedRoot 'install'
$data = Join-Path $ownedRoot 'data'
$testShortcut = Join-Path $ownedRoot 'shortcut/TaskCascade.lnk'
$realShortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'TaskCascade.lnk'
$dist = Join-Path $repoRoot 'frontend/dist'
$parkedDist = Join-Path $repoRoot "frontend/dist-install-rehearsal-$([guid]::NewGuid())"
$state = @{ child = $null; distParked = $false; sentinel = $null }

function Get-ShortcutRecord([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path)) { return 'absent' }
    $shell = New-Object -ComObject WScript.Shell
    $shortcut = $shell.CreateShortcut($Path)
    try {
        return ([ordered]@{ hash = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash; target = $shortcut.TargetPath } | ConvertTo-Json -Compress)
    }
    finally {
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($shortcut)
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($shell)
    }
}

function Stop-RehearsalCopy {
    if ($state.child) {
        Assert-That (-not $state.child.HasExited) 'Rehearsal child exited before a graceful close was requested.'
        Assert-That ($state.child.CloseMainWindow()) 'Rehearsal child did not accept the close request.'
        Assert-That ($state.child.WaitForExit(12000)) 'Rehearsal child did not close gracefully; forced cleanup is required.'
        $log = Get-Content -LiteralPath (Join-Path $data 'logs/taskcascade-desktop.log') -Raw
        Assert-That ($log -match "(?m)pid=$($state.child.Id) close: page confirmed all edits saved\r?$") 'Rehearsal page did not confirm all edits saved; the React UI may not have loaded.'
        $state.child.Dispose()
        $state.child = $null
    }
}

function Start-RehearsalCopy {
    # Child-only environment: never inherit a creation opt-in or real-data path.
    $info = [Diagnostics.ProcessStartInfo]::new((Join-Path $install 'taskcascade-desktop.exe'))
    $info.UseShellExecute = $false
    $info.WorkingDirectory = $install
    $info.Environment['TASKCASCADE_DATA_DIR'] = $data
    [void]$info.Environment.Remove('TASKCASCADE_CREATE_DATABASE')
    $state.child = [Diagnostics.Process]::Start($info)
    $logPath = Join-Path $data 'logs/taskcascade-desktop.log'
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    while ([DateTime]::UtcNow -lt $deadline) {
        $state.child.Refresh()
        if ($state.child.HasExited) { throw 'Rehearsal copy exited before its page loaded; another TaskCascade (dev) window may own se.pensjo.taskcascade.dev. Close it and retry.' }
        if ($state.child.MainWindowTitle -eq 'TaskCascade failure') { throw "Rehearsal copy reported a startup failure. Log: $(Get-Content -LiteralPath $logPath -Tail 10 | Out-String)" }
        if (Test-Path -LiteralPath $logPath) {
            $log = Get-Content -LiteralPath $logPath -Raw
            if ($log -match "(?m)pid=$($state.child.Id) eval self-test ok\r?$" -and $state.child.MainWindowHandle -ne 0) { return }
        }
        Start-Sleep -Milliseconds 100
    }
    throw 'Timed out waiting for this rehearsal process to log eval self-test ok and create its window.'
}

function Restore-FrontendDist {
    if ($state.distParked) {
        Assert-That (-not (Test-Path -LiteralPath $dist)) 'Cannot restore frontend/dist: another directory appeared there; parked build retained.'
        Move-Item -LiteralPath $parkedDist -Destination $dist
        $state.distParked = $false
    }
}

try {
    Invoke-Case '1 real Start-menu shortcut sentinel (read only)' {
        $state.sentinel = Get-ShortcutRecord $realShortcut
        foreach ($process in Get-Process -Name 'taskcascade-desktop' -ErrorAction SilentlyContinue) {
            Assert-That ($process.MainWindowTitle -notlike 'TaskCascade (dev)*') 'A TaskCascade (dev) window already owns se.pensjo.taskcascade.dev. Close it before rehearsing; it was left untouched.'
        }
    }
    Invoke-Case '2 install, embedded executable, shortcut and explicit database initialization' {
        $beforeEnvironment = @{}
        foreach ($name in @('TASKCASCADE_DATA_DIR', 'TASKCASCADE_CREATE_DATABASE')) { $beforeEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
        & $installer -InstallDir $install -ShortcutPath $testShortcut -DataDir $data -InitDatabase
        foreach ($name in $beforeEnvironment.Keys) {
            Assert-That ([Environment]::GetEnvironmentVariable($name, 'Process') -ceq $beforeEnvironment[$name]) "Installer changed $name in its caller."
        }
        $executable = Join-Path $install 'taskcascade-desktop.exe'
        Assert-That (Test-Path -LiteralPath $executable -PathType Leaf) 'Installed executable missing.'
        $manifest = Get-Content -LiteralPath (Join-Path $install 'install-info.txt') -Raw
        Assert-That ($manifest -match 'git HEAD: [0-9a-f]{40}' -and $manifest -match 'dirty: (True|False)' -and $manifest -match 'installed UTC: .+Z') 'Install info missing revision, dirty flag or UTC timestamp.'
        $record = Get-ShortcutRecord $testShortcut | ConvertFrom-Json
        Assert-That ($record.target -eq $executable) 'Test shortcut target differs.'
        Assert-That (Test-Path -LiteralPath (Join-Path $data 'taskcascade.sqlite') -PathType Leaf) 'Scratch database was not created.'
        $assetBytes = (Get-ChildItem -LiteralPath $dist -Recurse -File | Measure-Object -Property Length -Sum).Sum
        Assert-That ((Get-Item -LiteralPath $executable).Length -gt $assetBytes -and $assetBytes -gt 0) 'Executable size does not reflect the embedded frontend.'
        Assert-That ((Get-FileHash -LiteralPath $executable).Hash -eq (Get-FileHash -LiteralPath (Join-Path $repoRoot 'target/release/taskcascade-desktop.exe')).Hash) 'Installed executable differs from release build.'
        Assert-That (-not @(Get-Process -Name 'taskcascade-desktop' -ErrorAction SilentlyContinue | Where-Object Path -eq $executable).Count) 'Initialization left its child running.'
    }
    Invoke-Case '3 running installed copy refuses update without changes' {
        try {
            Start-RehearsalCopy
            $before = Get-TreeRecord $install
            $shortcutBefore = Get-ShortcutRecord $testShortcut
            Assert-Refused { & $installer -InstallDir $install -ShortcutPath $testShortcut -DataDir $data -SkipBuild } 'Close TaskCascade first'
            Assert-That ((Get-TreeRecord $install) -eq $before -and (Get-ShortcutRecord $testShortcut) -eq $shortcutBefore) 'Refused install changed the installed copy or shortcut.'
        }
        finally { Stop-RehearsalCopy }
    }
    Invoke-Case '4 self-contained UI with frontend/dist temporarily absent' {
        Assert-That ((Resolve-CanonicalPath $dist) -eq $dist -and (Split-Path -Parent $parkedDist) -eq (Split-Path -Parent $dist)) 'Unexpected frontend/dist path; refusing to move it.'
        Assert-That (-not (Test-Path -LiteralPath $parkedDist)) 'Parked dist path already exists.'
        Move-Item -LiteralPath $dist -Destination $parkedDist
        $state.distParked = $true
        try { Start-RehearsalCopy }
        finally { try { Stop-RehearsalCopy } finally { Restore-FrontendDist } }
    }
    Invoke-Case '5 -NoShortcut creates no shortcut under the root' {
        Remove-Item -LiteralPath $testShortcut
        Assert-That (-not @(Get-ChildItem -LiteralPath $ownedRoot -Recurse -File -Filter '*.lnk').Count) 'Shortcut baseline is not empty.'
        $install2 = Join-Path $ownedRoot 'install2'
        $database = Join-Path $data 'taskcascade.sqlite'
        $databaseHash = (Get-FileHash -LiteralPath $database -Algorithm SHA256).Hash
        $output = & $installer -InstallDir $install2 -ShortcutPath (Join-Path $ownedRoot 'shortcut2/TaskCascade.lnk') -DataDir $data -SkipBuild -NoShortcut -InitDatabase 6>&1 | Out-String
        Write-Host $output.TrimEnd()
        Assert-That ($output.Contains('Database already exists; -InitDatabase did nothing:')) 'Existing database did not skip child initialization.'
        Assert-That ((Get-FileHash -LiteralPath $database -Algorithm SHA256).Hash -eq $databaseHash) '-InitDatabase changed the existing scratch database.'
        Assert-That (Test-Path -LiteralPath (Join-Path $install2 'taskcascade-desktop.exe')) 'Second installed executable missing.'
        Assert-That (-not @(Get-ChildItem -LiteralPath $ownedRoot -Recurse -File -Filter '*.lnk').Count) '-NoShortcut created a shortcut.'
    }
}
finally {
    try {
        if ($state.child) {
            try {
                if (-not $state.child.HasExited) {
                    $state.child.Kill($true)
                    Assert-That ($state.child.WaitForExit(5000)) 'Rehearsal child did not stop during cleanup.'
                }
            }
            finally { $state.child.Dispose(); $state.child = $null }
        }
    }
    finally {
        try { Restore-FrontendDist }
        finally {
            Invoke-Case '6 cleanup and real Start-menu sentinel unchanged' {
                Remove-RehearsalRoot $ownedRoot
                Assert-That (-not (Test-Path -LiteralPath $ownedRoot)) 'Rehearsal root remains.'
                if ($null -ne $state.sentinel) { Assert-That ((Get-ShortcutRecord $realShortcut) -ceq $state.sentinel) 'Real Start-menu shortcut changed.' }
            }
        }
    }
}
