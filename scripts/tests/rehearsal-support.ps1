#Requires -Version 7.0
# Shared safety and assertions for synthetic Phase 1 rehearsals.
$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path

if (-not ('TaskCascade.RehearsalPaths' -as [type])) {
    Add-Type @'
using System.Runtime.InteropServices;
using System.Text;
namespace TaskCascade {
    public static class RehearsalPaths {
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        public static extern uint GetLongPathName(string shortPath, StringBuilder longPath, uint length);
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        public static extern uint GetShortPathName(string longPath, StringBuilder shortPath, uint length);
    }
}
'@
}

function Resolve-CanonicalPath([string]$Path) {
    $absolute = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Path)
    $driveRoot = [IO.Path]::GetPathRoot($absolute)
    $resolved = $driveRoot
    foreach ($part in $absolute.Substring($driveRoot.Length).Split([char[]]@('\', '/'), [StringSplitOptions]::RemoveEmptyEntries)) {
        $resolved = Join-Path $resolved $part
        if (Test-Path -LiteralPath $resolved) {
            $item = Get-Item -LiteralPath $resolved -Force
            if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { $resolved = $item.ResolveLinkTarget($true).FullName }
            $longPath = [Text.StringBuilder]::new(32768)
            $length = [TaskCascade.RehearsalPaths]::GetLongPathName($resolved, $longPath, $longPath.Capacity)
            if ($length -eq 0 -or $length -ge $longPath.Capacity) { throw "Cannot expand rehearsal path: $resolved" }
            $resolved = $longPath.ToString()
        }
    }
    return $resolved.TrimEnd('\', '/')
}

function Initialize-RehearsalRoot([string]$Path) {
    $absolute = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Path).TrimEnd('\', '/')
    $protected = @(Join-Path $repoRoot '.local/data')
    if ($env:LOCALAPPDATA) {
        $protected += Join-Path $env:LOCALAPPDATA 'TaskCascade'
        $protected += Join-Path $env:LOCALAPPDATA 'Programs/TaskCascade'
    }
    $protected += [Environment]::GetFolderPath('Programs')
    # Check spelling before resolving aliases, then check filesystem targets too.
    foreach ($candidate in @($absolute, (Resolve-CanonicalPath $absolute))) {
        foreach ($location in $protected) {
            foreach ($boundary in @($location, (Resolve-CanonicalPath $location))) {
                $boundary = $boundary.TrimEnd('\', '/')
                if ($candidate -eq $boundary -or $candidate.StartsWith("$boundary\", [StringComparison]::OrdinalIgnoreCase) -or
                    $boundary.StartsWith("$candidate\", [StringComparison]::OrdinalIgnoreCase)) { throw "Unsafe rehearsal root: $Path" }
            }
        }
    }
    $absolute = Resolve-CanonicalPath $absolute
    if (Test-Path -LiteralPath $absolute) {
        if (-not (Test-Path -LiteralPath $absolute -PathType Container) -or @(Get-ChildItem -LiteralPath $absolute -Force).Count) { throw 'Rehearsal root must be absent or empty.' }
    }
    New-Item -ItemType Directory -Force -Path $absolute | Out-Null
    return $absolute
}

function Remove-RehearsalRoot([string]$Path) {
    # Only delete the absolute canonical root that this invocation created.
    if ($Path -ne $script:ownedRoot -or (Resolve-CanonicalPath $Path) -ne $script:ownedRoot) { throw 'Rehearsal cleanup path changed.' }
    Remove-Item -LiteralPath $Path -Recurse -Force
}

function Assert-That([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

function Invoke-Case([string]$Name, [scriptblock]$Action) {
    try { & $Action; Write-Host "PASS $Name" -ForegroundColor Green }
    catch { Write-Host "FAIL $Name - $($_.Exception.Message)" -ForegroundColor Red; throw }
}

function Assert-Refused([scriptblock]$Action, [string]$Expected) {
    $message = $null
    try { & $Action } catch { $message = $_.Exception.Message }
    Assert-That ($null -ne $message -and $message -like "*$Expected*") "Expected refusal containing '$Expected', got '$message'."
}

function Invoke-RehearsalPowerShell([string]$Script, [string[]]$Arguments, [hashtable]$Environment) {
    $info = [Diagnostics.ProcessStartInfo]::new((Get-Command pwsh -ErrorAction Stop).Source)
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    foreach ($argument in @('-NoProfile', '-File', $Script) + $Arguments) { $info.ArgumentList.Add($argument) }
    foreach ($name in $Environment.Keys) { $info.Environment[$name] = $Environment[$name] }
    $process = [Diagnostics.Process]::Start($info)
    try {
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        $process.WaitForExit()
        return [pscustomobject]@{ ExitCode = $process.ExitCode; Output = $stdout.GetAwaiter().GetResult() + $stderr.GetAwaiter().GetResult() }
    }
    finally { $process.Dispose() }
}

function Invoke-Backend([string[]]$Arguments) {
    Push-Location (Join-Path $repoRoot 'backend')
    try {
        $output = & cargo @Arguments
        if ($LASTEXITCODE -ne 0) { throw "cargo $($Arguments -join ' ') failed." }
        return $output
    }
    finally { Pop-Location }
}

function New-Fixture([string]$Directory, [string]$Kind) {
    Invoke-Backend @('run', '--quiet', '--example', 'migration_fixture', '--', $Directory, $Kind) | Out-Null
}

function Get-Summary([string]$Path) {
    return (Invoke-Backend @('run', '--quiet', '--example', 'database_summary', '--', $Path) | ConvertFrom-Json | ConvertTo-Json -Compress)
}

function Get-TreeRecord([string]$Directory) {
    return (@(Get-ChildItem -LiteralPath $Directory -File -Recurse -Force | Sort-Object FullName | ForEach-Object {
        [ordered]@{ path = [IO.Path]::GetRelativePath($Directory, $_.FullName); size = $_.Length; modified = $_.LastWriteTimeUtc.Ticks; hash = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash }
    }) | ConvertTo-Json -Depth 4 -Compress)
}
