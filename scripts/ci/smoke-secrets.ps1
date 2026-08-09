<#
.SYNOPSIS
    Secret detection and redacted export, against a fixture that exists only
    for this check.

.DESCRIPTION
    `ct secrets` must report a kind and a location and never the matched text.
    `ct export --redact-secrets` must remove every credential and say on stderr
    how many it changed.

    The credential fixture lives under its own isolated CODEX_HOME, never the
    one smoke-cli.ps1 uses, so this cannot be satisfied by the compaction
    fixture happening to contain a credential shape too.

.PARAMETER CtExe
    Path to the release ct.exe. See common.ps1 for the resolution order.
#>
[CmdletBinding()]
param([string]$CtExe)

. (Join-Path $PSScriptRoot 'common.ps1')

$repoRoot = Get-RepoRoot
$script:CtExecutable = Resolve-CtExecutable -CtExe $CtExe

$root = New-CiWorkingRoot -Name 'secrets'
$homes = Use-IsolatedAgentHomes -Root $root

$codexDay = New-CleanDirectory (Join-Path $homes.Codex 'sessions/2026/07/25')
Copy-Item (Join-Path $repoRoot 'tests/fixtures/secrets/codex/rollout.jsonl') (Join-Path $codexDay 'rollout.jsonl')

$sessions = @(Invoke-CtJson -Arguments @('sessions', '--json'))
$codex = @($sessions | Where-Object { $_.agent -eq 'codex' })
if ($codex.Count -ne 1) { throw 'Expected exactly the dedicated credential fixture under the isolated CODEX_HOME.' }
$id = $codex[0].id

$secretsText = (Invoke-Ct -Arguments @('secrets', $id)) -join "`n"
if ($secretsText -notmatch 'Found (\d+) potential secret occurrence') {
    throw 'Expected ct secrets to report findings against the credential fixture.'
}
if ([int]$Matches[1] -le 0) { throw 'Expected a non-zero secret finding count.' }

foreach ($label in @('OpenAI API key', 'Anthropic API key', 'GitHub token', 'AWS access key id', 'private key')) {
    if ($secretsText -notmatch [regex]::Escape($label)) { throw "Expected a '$label' finding." }
}
Assert-NoFixtureSecretIn -Text $secretsText -Because 'ct secrets must never print a matched credential value.'

# Positive control, taken before the redacted export it validates. Establishes
# which credentials this exporter emits at all, so the assertion below is known
# to be capable of failing rather than merely observed to pass.
$plainExport = (Invoke-Ct -Arguments @('export', $id)) -join "`n"
$exportable = Get-FixtureSecretsPresentIn -Text $plainExport
if ($exportable.Count -lt 1) {
    throw 'No fixture credential appears in an unredacted export, so the redaction assertion below would be vacuous. Either the fixture or ct export changed.'
}

$exportStderr = Join-Path $root 'export-secrets-stderr.txt'
$exportText = (Invoke-Ct -Arguments @('export', $id, '--redact-secrets') -StandardErrorPath $exportStderr) -join "`n"
if ($exportText -notmatch '"redaction":"secrets"') {
    throw 'Expected the export header to record that secrets redaction ran.'
}
Assert-NoFixtureSecretIn -Text $exportText -Secrets $exportable -Because 'Redacted export still contains a credential that an unredacted export emitted.'
# Belt and braces: the full list too, in case the exporter widens later and this
# script runs before anyone updates the positive control.
Assert-NoFixtureSecretIn -Text $exportText -Because 'Redacted export still contains a credential.'

$exportStderrText = Get-Content $exportStderr -Raw
if ($exportStderrText -notmatch 'Redacted (\d+) potential secret occurrence') {
    throw 'Expected a redaction summary on stderr.'
}
if ([int]$Matches[1] -le 0) { throw 'Expected the redacted export to have changed at least one field.' }

Write-Host "OK: every credential kind reported, none printed, and none of the $($exportable.Count) export-reachable credential(s) surviving the redacted export."
