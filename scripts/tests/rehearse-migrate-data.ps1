#Requires -Version 7.0
[CmdletBinding()]
param([Parameter(Mandatory)][ValidateNotNullOrEmpty()][string]$Root)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'rehearsal-support.ps1')
$ownedRoot = Initialize-RehearsalRoot $Root
$migrate = Join-Path $repoRoot 'scripts/migrate-data.ps1'

function Get-ProtectedDataMetadata {
    # Metadata only: never hash, copy, summarize, or open these files.
    $directories = @(Join-Path $repoRoot '.local/data')
    if ($env:LOCALAPPDATA) { $directories += Join-Path $env:LOCALAPPDATA 'TaskCascade' }
    return (@(foreach ($directory in $directories) {
        if (Test-Path -LiteralPath $directory) {
            Get-ChildItem -LiteralPath $directory -Filter 'taskcascade.sqlite*' -File -Force |
                Sort-Object Name | ForEach-Object {
                    [ordered]@{ path = $_.FullName; exists = $true; size = $_.Length; modified = $_.LastWriteTimeUtc.Ticks }
                }
        }
    }) | ConvertTo-Json -Depth 4 -Compress)
}

function Get-DatabaseHashes([string]$Directory) {
    return (@(Get-ChildItem -LiteralPath $Directory -Filter 'taskcascade.sqlite*' -File | Sort-Object Name | ForEach-Object {
        "$($_.Name):$($_.Length):$((Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash)"
    }) -join "`n")
}

try {
    $realBefore = Get-ProtectedDataMetadata
    if (Get-Process -Name 'taskcascade-backend', 'taskcascade-desktop' -ErrorAction SilentlyContinue) {
        throw 'Migration rehearsal requires every TaskCascade backend and desktop process to be closed, including the daily app. Close the running TaskCascade backend or desktop app, then re-run.'
    }
    Invoke-Case '1 WAL fixture migrates completely' {
        $source = Join-Path $ownedRoot 'wal/src'
        $target = Join-Path $ownedRoot 'wal/dst'
        New-Fixture $source 'wal'
        $expected = Get-Content -LiteralPath (Join-Path $source 'expected.json') -Raw | ConvertFrom-Json | ConvertTo-Json -Compress
        $originalHashes = Get-DatabaseHashes $source
        & $migrate -SourcePath (Join-Path $source 'taskcascade.sqlite') -TargetRoot $target
        Assert-That ((Get-DatabaseHashes $target) -eq $originalHashes) 'Moved hashes differ.'
        Assert-That ((Get-DatabaseHashes (Join-Path $target 'pre-move-backup')) -eq $originalHashes) 'Safety copy differs.'
        Assert-That (-not @(Get-ChildItem -LiteralPath $source -Filter 'taskcascade.sqlite*').Count) 'Source database files remain.'
        # Summarize a disposable copy so this assertion cannot checkpoint dst.
        $copy = Join-Path $ownedRoot 'wal/check'
        New-Item -ItemType Directory -Path $copy | Out-Null
        Get-ChildItem -LiteralPath $target -Filter 'taskcascade.sqlite*' -File | Copy-Item -Destination $copy
        Assert-That ((Get-Summary (Join-Path $copy 'taskcascade.sqlite')) -eq $expected) 'WAL summary differs from expected.json.'
    }
    Invoke-Case '2 fixture really needs its WAL' {
        $copy = Join-Path $ownedRoot 'main-only'
        New-Item -ItemType Directory -Path $copy | Out-Null
        Copy-Item -LiteralPath (Join-Path $ownedRoot 'wal/dst/taskcascade.sqlite') -Destination $copy
        $actual = Get-Summary (Join-Path $copy 'taskcascade.sqlite') | ConvertFrom-Json
        $expected = Get-Content -LiteralPath (Join-Path $ownedRoot 'wal/src/expected.json') -Raw | ConvertFrom-Json
        Assert-That ($actual.activeTasks -eq $expected.activeTasks - 1 -and $actual.newestModifiedAt -ne $expected.newestModifiedAt) 'Main file alone contains the sentinel.'
    }
    Invoke-Case '3 plain fixture migrates' {
        $source = Join-Path $ownedRoot 'plain/src'
        $target = Join-Path $ownedRoot 'plain/dst'
        New-Fixture $source 'plain'
        $expected = Get-Content -LiteralPath (Join-Path $source 'expected.json') -Raw | ConvertFrom-Json | ConvertTo-Json -Compress
        & $migrate -SourcePath (Join-Path $source 'taskcascade.sqlite') -TargetRoot $target
        Assert-That ((Get-Summary (Join-Path $target 'taskcascade.sqlite')) -eq $expected) 'Plain summary differs.'
    }
    Invoke-Case '4 existing target is refused without changes' {
        $directory = Join-Path $ownedRoot 'plain'
        $before = Get-TreeRecord $directory
        Assert-Refused { & $migrate -SourcePath (Join-Path $directory 'src/taskcascade.sqlite') -TargetRoot (Join-Path $directory 'dst') } 'target already exists'
        Assert-That ((Get-TreeRecord $directory) -eq $before) 'Refusal changed files.'
    }
    Invoke-Case '5 locked source is refused' {
        $source = Join-Path $ownedRoot 'locked/src'
        New-Fixture $source 'plain'
        $path = Join-Path $source 'taskcascade.sqlite'
        $ready = Join-Path $ownedRoot 'locked/ready'
        $job = Start-Job -ArgumentList $path, $ready -ScriptBlock {
            param($Path, $Ready)
            $ErrorActionPreference = 'Stop'
            $stream = [IO.File]::Open($Path, 'Open', 'ReadWrite', 'None')
            try { [IO.File]::WriteAllText($Ready, 'ready'); while ($true) { Start-Sleep -Milliseconds 200 } }
            finally { $stream.Dispose() }
        }
        try {
            $deadline = [DateTime]::UtcNow.AddSeconds(20)
            while (-not (Test-Path -LiteralPath $ready) -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 100 }
            Assert-That (Test-Path -LiteralPath $ready) 'Lock job did not become ready.'
            Assert-Refused { & $migrate -SourcePath $path -TargetRoot (Join-Path $ownedRoot 'locked/dst') } 'exclusively'
        }
        finally { Stop-Job $job; Remove-Job $job }
    }
    Invoke-Case '6 hot journal is refused' {
        $source = Join-Path $ownedRoot 'journal/src'
        New-Fixture $source 'plain'
        $path = Join-Path $source 'taskcascade.sqlite'
        [IO.File]::WriteAllBytes("$path-journal", [byte[]]@())
        $before = Get-TreeRecord $source
        Assert-Refused { & $migrate -SourcePath $path -TargetRoot (Join-Path $ownedRoot 'journal/dst') } 'hot journal'
        Assert-That ((Get-TreeRecord $source) -eq $before) 'Journal refusal changed source.'
    }
    Invoke-Case '7 forced mid-move failure restores the source set' {
        $source = Join-Path $ownedRoot 'failure/src'
        $target = Join-Path $ownedRoot 'failure/dst'
        New-Fixture $source 'wal'
        $before = Get-DatabaseHashes $source
        Assert-Refused { & $migrate -SourcePath (Join-Path $source 'taskcascade.sqlite') -TargetRoot $target -SimulateFailureAfterMoves 1 } 'source set is intact'
        Assert-That ((Get-DatabaseHashes $source) -eq $before) 'Rollback changed source hashes.'
        Assert-That (-not @(Get-ChildItem -LiteralPath $target -Filter 'taskcascade.sqlite*' -File).Count) 'Rollback left target database files.'
        Assert-That ((Get-DatabaseHashes (Join-Path $target 'pre-move-backup')) -eq $before) 'Rollback safety copy differs.'
    }
    foreach ($parameter in @('SourcePath', 'TargetRoot')) {
        Invoke-Case "one-sided -$parameter is refused with synthetic defaults" {
            $fakeRepo = Join-Path $ownedRoot "one-sided-$parameter/fakerepo"
            $fakeScripts = Join-Path $fakeRepo 'scripts'
            $fakeAppData = Join-Path $ownedRoot "one-sided-$parameter/localappdata"
            $syntheticTarget = Join-Path $ownedRoot "one-sided-$parameter/target"
            New-Item -ItemType Directory -Path $fakeScripts, $fakeAppData | Out-Null
            # The migration script has no dot-sourced dependencies. Both of its
            # defaults are synthetic even if the paired-path guard regresses.
            $copy = Join-Path $fakeScripts 'migrate-data.ps1'
            Copy-Item -LiteralPath $migrate -Destination $copy
            $fakeSource = Join-Path $fakeRepo '.local/data'
            New-Fixture $fakeSource 'plain'
            $before = Get-TreeRecord (Split-Path -Parent $fakeRepo)
            $value = if ($parameter -eq 'SourcePath') { Join-Path $fakeSource 'taskcascade.sqlite' } else { $syntheticTarget }
            $result = Invoke-RehearsalPowerShell $copy @("-$parameter", $value) @{ LOCALAPPDATA = $fakeAppData }
            Assert-That ($result.ExitCode -ne 0 -and $result.Output -like '*Provide both -SourcePath and -TargetRoot, or neither*') "Expected paired-path refusal, got $($result.Output)"
            Assert-That ((Get-TreeRecord (Split-Path -Parent $fakeRepo)) -eq $before) 'One-sided refusal changed files.'
            Assert-That (Test-Path -LiteralPath (Join-Path $fakeSource 'taskcascade.sqlite')) 'Fake default source disappeared.'
            Assert-That (-not (Test-Path -LiteralPath (Join-Path $fakeAppData 'TaskCascade'))) 'Fake default target was created.'
            Assert-That (-not (Test-Path -LiteralPath $syntheticTarget)) 'Explicit synthetic target was created.'
        }
    }
    Invoke-Case 'failure hook beyond the source count is refused before changes' {
        $source = Join-Path $ownedRoot 'oversized-hook/src'
        $target = Join-Path $ownedRoot 'oversized-hook/dst'
        New-Fixture $source 'plain'
        $before = Get-TreeRecord $source
        Assert-Refused { & $migrate -SourcePath (Join-Path $source 'taskcascade.sqlite') -TargetRoot $target -SimulateFailureAfterMoves 2 } 'cannot exceed the source file count'
        Assert-That ((Get-TreeRecord $source) -eq $before) 'Invalid failure hook changed source.'
        Assert-That (-not (Test-Path -LiteralPath $target)) 'Invalid failure hook created a target.'
    }
    Invoke-Case 'temporary cleanup failure preserves the split-state error' {
        $ast = [Management.Automation.Language.Parser]::ParseFile($migrate, [ref]$null, [ref]$null)
        $migrationTry = $ast.EndBlock.Statements | Where-Object { $_ -is [Management.Automation.Language.TryStatementAst] } | Select-Object -Last 1
        $cleanup = [scriptblock]::Create(($migrationTry.Finally.Statements.Extent.Text -join "`n"))
        # Exercise the actual finalizer with a failing deletion, without moving
        # a database or leaving deliberately locked temporary copies behind.
        function Remove-Item { throw 'simulated temporary cleanup failure' }
        $tempRoot = Join-Path $ownedRoot 'unused-cleanup-test'
        $warnings = @()
        Assert-Refused {
            try { throw 'SPLIT DATABASE STATE: recover from pre-move-backup' }
            finally { $warnings = @(& $cleanup 3>&1); Assert-That ($warnings.Count -eq 1 -and $warnings[0] -like '*simulated temporary cleanup failure*') 'Cleanup failure was not reported as a warning.' }
        } 'SPLIT DATABASE STATE: recover from pre-move-backup'
    }
    Invoke-Case 'relative migration paths use the PowerShell location' {
        $directory = Join-Path $ownedRoot 'relative'
        New-Fixture (Join-Path $directory 'src') 'plain'
        Push-Location $directory
        try { & $migrate -SourcePath 'src/taskcascade.sqlite' -TargetRoot 'dst' }
        finally { Pop-Location }
        Assert-That (Test-Path -LiteralPath (Join-Path $directory 'dst/taskcascade.sqlite')) 'Relative target was resolved elsewhere.'
        Assert-That (-not (Test-Path -LiteralPath (Join-Path $directory 'src/taskcascade.sqlite'))) 'Relative source was not moved.'
    }
    Invoke-Case '8.3 aliases cannot bypass the rehearsal-root guard' {
        $fakeAppData = Join-Path $ownedRoot 'short-path-localappdata'
        $protected = Join-Path $fakeAppData 'TaskCascade'
        New-Item -ItemType Directory -Path $protected | Out-Null
        $shortPath = [Text.StringBuilder]::new(32768)
        $length = [TaskCascade.RehearsalPaths]::GetShortPathName($protected, $shortPath, $shortPath.Capacity)
        Assert-That ($length -gt 0 -and $length -lt $shortPath.Capacity) 'Cannot obtain synthetic short path.'
        Assert-That ((Resolve-CanonicalPath $shortPath.ToString()) -eq (Resolve-CanonicalPath $protected)) 'Short path was not expanded.'
        $previousLocalAppData = $env:LOCALAPPDATA
        try {
            $env:LOCALAPPDATA = $fakeAppData
            Assert-Refused { Initialize-RehearsalRoot (Join-Path $shortPath.ToString() 'absent-child') } 'Unsafe rehearsal root'
        }
        finally { $env:LOCALAPPDATA = $previousLocalAppData }
        if ($shortPath.ToString() -eq $protected) { Write-Host '8.3 aliases are disabled on this volume; canonical guard checked with the long path.' }
    }
    Invoke-Case '8 real-data paths untouched (metadata only)' {
        Assert-That ((Get-ProtectedDataMetadata) -eq $realBefore) 'Real-data metadata changed.'
    }
}
finally { Remove-RehearsalRoot $ownedRoot }
