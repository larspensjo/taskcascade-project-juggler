#Requires -Version 7.0
# Adapted from TickerTapeTallyBoard/scripts/migrate-ledger.ps1 (42f8bf4).
[CmdletBinding()]
param(
    [ValidateNotNullOrEmpty()][string]$SourcePath,
    [ValidateNotNullOrEmpty()][string]$TargetRoot,
    [ValidateRange(1, 3)][int]$SimulateFailureAfterMoves
)

$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$explicitPaths = $PSBoundParameters.ContainsKey('SourcePath') -and $PSBoundParameters.ContainsKey('TargetRoot')
$defaultPaths = -not $PSBoundParameters.ContainsKey('SourcePath') -and -not $PSBoundParameters.ContainsKey('TargetRoot')
if (-not $explicitPaths -and -not $defaultPaths) {
    throw 'Provide both -SourcePath and -TargetRoot, or neither.'
}
if ($explicitPaths -and ([string]::IsNullOrWhiteSpace($SourcePath) -or [string]::IsNullOrWhiteSpace($TargetRoot))) {
    throw '-SourcePath and -TargetRoot must not be blank.'
}
if ($SimulateFailureAfterMoves -and (-not $explicitPaths -or [string]::IsNullOrWhiteSpace($SourcePath) -or [string]::IsNullOrWhiteSpace($TargetRoot))) {
    throw '-SimulateFailureAfterMoves requires explicit -SourcePath and -TargetRoot.'
}
if ([string]::IsNullOrWhiteSpace($SourcePath)) { $SourcePath = Join-Path $repoRoot '.local/data/taskcascade.sqlite' }
if ([string]::IsNullOrWhiteSpace($TargetRoot)) {
    if ([string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) { throw 'LOCALAPPDATA is unset; provide -TargetRoot.' }
    # Other definition of this default: desktop/src/data_location.rs (Phase 4).
    $TargetRoot = Join-Path $env:LOCALAPPDATA 'TaskCascade'
}
$SourcePath = $PSCmdlet.GetUnresolvedProviderPathFromPSPath($SourcePath)
$TargetRoot = $PSCmdlet.GetUnresolvedProviderPathFromPSPath($TargetRoot)
$targetPath = Join-Path $TargetRoot 'taskcascade.sqlite'
$backupRoot = Join-Path $TargetRoot 'pre-move-backup'

function Get-Fingerprint([string]$Path) {
    $item = Get-Item -LiteralPath $Path -Force
    if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw "Expected a regular file: $Path" }
    return "$($item.Length):$((Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash)"
}

function Get-CopySummary([string]$Directory, [string]$FromRoot) {
    New-Item -ItemType Directory -Path $Directory | Out-Null
    foreach ($file in $files) {
        $from = if ($FromRoot) { Join-Path $FromRoot $file.Name } else { $file.Source }
        Copy-Item -LiteralPath $from -Destination (Join-Path $Directory $file.Name)
    }
    Push-Location (Join-Path $repoRoot 'backend')
    try {
        $json = & cargo run --quiet --manifest-path (Join-Path $repoRoot 'backend/Cargo.toml') --example database_summary -- (Join-Path $Directory 'taskcascade.sqlite')
        if ($LASTEXITCODE -ne 0) { throw 'database_summary failed on the temporary copy.' }
        $summary = $json | ConvertFrom-Json
        if ($summary.integrity -ne 'ok') { throw "Database integrity check failed: $($summary.integrity)" }
        return ($summary | ConvertTo-Json -Compress)
    }
    finally { Pop-Location }
}

if (Get-Process -Name 'taskcascade-backend', 'taskcascade-desktop' -ErrorAction SilentlyContinue) {
    throw 'Refusing migration: a TaskCascade backend or desktop process is running (including the daily app). Close the running TaskCascade backend or desktop app, then re-run.'
}
foreach ($suffix in @('', '-wal', '-shm', '-journal')) {
    if (Test-Path -LiteralPath "$targetPath$suffix") { throw "Refusing migration: target already exists: $targetPath$suffix" }
}
if (-not (Test-Path -LiteralPath $SourcePath -PathType Leaf)) { throw "Source database is missing or not a regular file: $SourcePath" }
if (Test-Path -LiteralPath "$SourcePath-journal") {
    throw 'Refusing hot journal: start and close the old app once so SQLite recovers it, then rerun.'
}
$files = @()
foreach ($suffix in @('', '-wal', '-shm')) {
    $path = "$SourcePath$suffix"
    if (-not (Test-Path -LiteralPath $path)) { continue }
    $stream = $null
    try { $stream = [IO.File]::Open($path, 'Open', 'ReadWrite', 'None') }
    catch { throw "Refusing migration: source file cannot be opened exclusively: $path. Close the application first. $($_.Exception.Message)" }
    finally { if ($stream) { $stream.Dispose() } }
    $files += [pscustomobject]@{ Source = $path; Name = "taskcascade.sqlite$suffix"; Fingerprint = (Get-Fingerprint $path) }
}
if ($SimulateFailureAfterMoves -gt $files.Count) {
    throw "-SimulateFailureAfterMoves cannot exceed the source file count ($($files.Count)). No move attempted."
}

$tempRoot = Join-Path ([IO.Path]::GetTempPath()) "taskcascade-migrate-$([guid]::NewGuid())"
New-Item -ItemType Directory -Path $tempRoot | Out-Null
try {
    $before = Get-CopySummary (Join-Path $tempRoot 'source') ''
    Write-Host "Source summary: $before"
    if (Test-Path -LiteralPath $backupRoot) {
        if (-not (Test-Path -LiteralPath $backupRoot -PathType Container)) { throw "Safety copy path is not a directory: $backupRoot" }
        $contents = @(Get-ChildItem -LiteralPath $backupRoot -Force)
        if ($contents.Count -gt 0) {
            $identical = $contents.Count -eq $files.Count
            foreach ($file in $files) {
                $backup = Join-Path $backupRoot $file.Name
                if (-not (Test-Path -LiteralPath $backup -PathType Leaf) -or (Get-Fingerprint $backup) -ne $file.Fingerprint) { $identical = $false }
            }
            if (-not $identical) { throw "Existing safety copy differs from the source: $backupRoot. No move attempted; retain it until you have confirmed a complete source set." }
        }
    }
    New-Item -ItemType Directory -Force -Path $backupRoot | Out-Null
    foreach ($file in $files) {
        $backup = Join-Path $backupRoot $file.Name
        if (-not (Test-Path -LiteralPath $backup)) { [IO.File]::Copy($file.Source, $backup, $false) }
        if ((Get-Fingerprint $file.Source) -ne $file.Fingerprint -or (Get-Fingerprint $backup) -ne $file.Fingerprint) {
            throw "Safety copy verification failed or source changed: $($file.Source). No move attempted."
        }
    }
    $moved = @()
    try {
        # Move sidecars first, main file last; no destination is ever overwritten.
        foreach ($file in @($files | Select-Object -Skip 1) + @($files[0])) {
            $destination = [IO.Path]::GetFullPath((Join-Path $TargetRoot $file.Name))
            if ((Split-Path -Parent $destination) -ne $TargetRoot.TrimEnd('\', '/')) { throw 'Move destination escaped target root.' }
            Move-Item -LiteralPath $file.Source -Destination $destination
            $moved += $file
            if ($SimulateFailureAfterMoves -and $moved.Count -eq $SimulateFailureAfterMoves) { throw 'Simulated mid-move failure.' }
        }
    }
    catch {
        $moveError = $_.Exception.Message
        $rollbackErrors = @()
        for ($index = $moved.Count - 1; $index -ge 0; $index--) {
            $file = $moved[$index]
            try {
                $destination = [IO.Path]::GetFullPath($file.Source)
                if ((Split-Path -Parent $destination) -ne (Split-Path -Parent $SourcePath)) { throw 'Rollback path escaped source directory.' }
                if (Test-Path -LiteralPath $destination) { throw 'Source name already exists.' }
                Move-Item -LiteralPath (Join-Path $TargetRoot $file.Name) -Destination $destination
            }
            catch { $rollbackErrors += $_.Exception.Message }
        }
        foreach ($file in $files) {
            if (-not (Test-Path -LiteralPath $file.Source) -or (Test-Path -LiteralPath (Join-Path $TargetRoot $file.Name))) { $rollbackErrors += "Split-state artifact: $($file.Name)" }
            elseif ((Get-Fingerprint $file.Source) -ne $file.Fingerprint) { $rollbackErrors += "Restored hash mismatch: $($file.Name)" }
        }
        if ($rollbackErrors.Count) {
            throw "SPLIT DATABASE STATE: do not start TaskCascade. Recover the complete database and sidecars from pre-move-backup at $backupRoot. Move error: $moveError Rollback: $($rollbackErrors -join ' ')"
        }
        throw "Migration move failed: $moveError All moved files were restored; source set is intact. Safety copy: $backupRoot"
    }
    foreach ($file in $files) {
        $destination = Join-Path $TargetRoot $file.Name
        if ((Get-Fingerprint $destination) -ne $file.Fingerprint) { throw "MOVED FILE VERIFICATION FAILED: $destination. Do not start TaskCascade; recover from $backupRoot." }
        Write-Host "Moved: $destination ($($file.Fingerprint))"
    }
    $after = Get-CopySummary (Join-Path $tempRoot 'target') $TargetRoot
    if ($before -ne $after) { throw "CONTENT VERIFICATION FAILED. Do not start TaskCascade; recover from $backupRoot." }
    Write-Host "Verified summary: $after"
    Write-Host "Safety copy: $backupRoot"
}
finally {
    # tempRoot is an absolute, freshly generated directory, never a caller path.
    try { Remove-Item -LiteralPath $tempRoot -Recurse -Force }
    catch { Write-Warning "Could not remove temporary migration copies at ${tempRoot}: $($_.Exception.Message)" }
}
if ($defaultPaths) {
    $documents = @((Join-Path $env:USERPROFILE 'Documents'), [Environment]::GetFolderPath('MyDocuments')) | Select-Object -Unique
    foreach ($directory in $documents) {
        if (-not $directory) { continue }
        $path = Join-Path $directory 'TaskCascade/taskcascade.sqlite'
        if (Test-Path -LiteralPath $path) {
            $item = Get-Item -LiteralPath $path
            Write-Host "$path - $($item.Length) bytes - $($item.LastWriteTimeUtc.ToString('o')) - left untouched"
        }
    }
}
Write-Host 'instances\ was not touched.'
Write-Host 'Next: .\scripts\start.ps1 -CreateRealDataSnapshot'
