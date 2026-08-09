<#
.SYNOPSIS
    Discovery, inspection, context analysis and compaction-diff smoke over the
    committed synthetic fixtures.

.DESCRIPTION
    Proves the release CLI finds both agents' sessions, answers the three
    read-only queries the desktop app is built on, and reports a structurally
    diffable compaction with at least one dropped item.

    Runs against isolated agent homes, so it says nothing about -- and cannot be
    satisfied by -- the sessions on the machine executing it.

.PARAMETER CtExe
    Path to the release ct.exe. Defaults to CT_EXE, then to
    $CARGO_TARGET_DIR/release/ct.exe, then to <repo>/target/release/ct.exe.
#>
[CmdletBinding()]
param([string]$CtExe)

. (Join-Path $PSScriptRoot 'common.ps1')

$repoRoot = Get-RepoRoot
$script:CtExecutable = Resolve-CtExecutable -CtExe $CtExe
Write-Host "Using $script:CtExecutable"

$homes = Use-IsolatedAgentHomes -Root (New-CiWorkingRoot -Name 'cli')

$codexDay = New-CleanDirectory (Join-Path $homes.Codex 'sessions/2026/07/29')
$claudeProject = New-CleanDirectory (Join-Path $homes.Claude 'projects/ci-fixtures')
Copy-Item (Join-Path $repoRoot 'tests/fixtures/codex/rollout.jsonl') (Join-Path $codexDay 'rollout.jsonl')
Copy-Item (Join-Path $repoRoot 'tests/fixtures/claude_code/session.jsonl') (Join-Path $claudeProject 'session.jsonl')

$sessions = @(Invoke-CtJson -Arguments @('sessions', '--json'))
if ($sessions.Count -ne 2) { throw "Expected two fixture sessions, found $($sessions.Count)." }

$codex = @($sessions | Where-Object { $_.agent -eq 'codex' })
$claude = @($sessions | Where-Object { $_.agent -eq 'claude-code' })
if ($codex.Count -ne 1 -or $claude.Count -ne 1) { throw 'Both fixture agents must be discovered.' }
$codexId = $codex[0].id

if ($null -eq (Invoke-CtJson -Arguments @('inspect', $codexId, '--json'))) {
    throw 'Codex fixture inspection produced no JSON.'
}
if ($null -eq (Invoke-CtJson -Arguments @('context', $codexId, '--json'))) {
    throw 'Codex fixture context analysis produced no JSON.'
}

$compactions = @(Invoke-CtJson -Arguments @('compactions', $codexId, '--json'))
if ($compactions.Count -lt 1) { throw 'Expected at least one compaction event in the codex fixture.' }

$available = @($compactions | Where-Object { $_.status -eq 'available' })
if ($available.Count -lt 1) { throw 'Expected a structurally diffable compaction (replacement_history present).' }

$dropped = @($available[0].items | Where-Object { $_.disposition.kind -eq 'dropped' })
if ($dropped.Count -lt 1) { throw 'Expected the compaction to report at least one dropped item.' }

Write-Host "OK: 2 sessions discovered, inspect/context answered, $($dropped.Count) dropped item(s) in the compaction diff."
