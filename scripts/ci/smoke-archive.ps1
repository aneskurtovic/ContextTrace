<#
.SYNOPSIS
    Archiving, redaction on ingest, and verification.

.DESCRIPTION
    The archive is the one place ContextTrace writes, and it concentrates by
    construction what was scattered across a home directory. So the same fixture
    that proves `ct secrets` never prints a credential is used to prove the
    archive never stores one by default.

    This asserts on the bytes actually written to disk -- including the manifest
    -- not on what the command reported doing.

.PARAMETER CtExe
    Path to the release ct.exe. See common.ps1 for the resolution order.
#>
[CmdletBinding()]
param([string]$CtExe)

. (Join-Path $PSScriptRoot 'common.ps1')

$repoRoot = Get-RepoRoot
$script:CtExecutable = Resolve-CtExecutable -CtExe $CtExe

$root = New-CiWorkingRoot -Name 'archive'
$homes = Use-IsolatedAgentHomes -Root $root
$archiveRoot = $homes.Archive

$codexDay = New-CleanDirectory (Join-Path $homes.Codex 'sessions/2026/07/25')
$fixture = Join-Path $repoRoot 'tests/fixtures/secrets/codex/rollout.jsonl'
Copy-Item $fixture (Join-Path $codexDay 'rollout.jsonl')

# Positive control. The archive is a redacted copy of this file, so "none of
# these strings is in the archive" is only evidence if they are all in the
# input. Unlike the export path, every fixture credential is reachable here --
# the archive copies raw JSONL rather than rendering context-bearing records.
$fixtureText = Get-Content $fixture -Raw
$missing = @($script:FixtureSecrets | Where-Object { -not $fixtureText.Contains($_) })
if ($missing.Count -gt 0) {
    throw "The credential fixture no longer contains $($missing.Count) of the values this script asserts the archive strips, which would make those assertions vacuous."
}

$sessions = @(Invoke-CtJson -Arguments @('sessions', '--json'))
$codex = @($sessions | Where-Object { $_.agent -eq 'codex' })
if ($codex.Count -ne 1) { throw 'Expected exactly the dedicated credential fixture under the isolated CODEX_HOME.' }
$id = $codex[0].id

$roots = (Invoke-Ct -Arguments @('roots')) -join "`n"
if ($roots -notmatch [regex]::Escape($archiveRoot)) {
    throw 'ct roots must name the directory ContextTrace writes to.'
}

$archived = Invoke-CtJson -Arguments @('archive', $id, '--json')
if ($archived.redaction -ne 'redacted') { throw 'Archiving must redact by default.' }
if ($archived.redacted_values -le 0) {
    throw 'Expected the credential fixture to have values replaced on ingest.'
}

$storedText = (Get-ChildItem -Path $archiveRoot -Recurse -File | ForEach-Object { Get-Content $_.FullName -Raw }) -join "`n"
if ([string]::IsNullOrEmpty($storedText)) { throw 'The archive wrote nothing.' }
Assert-NoFixtureSecretIn -Text $storedText -Because 'A credential survived into the archive, including its manifest.'

# Every archived record must still be a parseable JSON line, or the copy is
# worse than useless -- redaction substitutes inside raw JSONL.
$copy = Get-ChildItem -Path (Join-Path $archiveRoot 'sessions') -Recurse -File | Select-Object -First 1
if ($null -eq $copy) { throw 'The archive stored no session file.' }
foreach ($line in (Get-Content $copy.FullName)) {
    if ([string]::IsNullOrWhiteSpace($line)) { continue }
    try { $null = $line | ConvertFrom-Json } catch { throw 'Redaction left an archived record unparseable.' }
}

$verify = (Invoke-Ct -Arguments @('archive', $id, '--verify')) -join "`n"
if ($verify -notmatch 'intact') { throw 'A freshly archived session must verify as intact.' }

$list = @(Invoke-CtJson -Arguments @('archive', '--json'))
if ($list.Count -ne 1) { throw "Expected exactly one archived session, found $($list.Count)." }

Write-Host "OK: redacted on ingest ($($archived.redacted_values) values), no credential on disk, records parseable, verify intact."
