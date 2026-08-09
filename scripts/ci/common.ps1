# Shared helpers for the ContextTrace CI smoke scripts. Dot-source, do not run.
#
# These assertions used to live inline in `.github/workflows/ci.yml`, as ~190
# lines of PowerShell embedded in YAML. They are here as real script files for
# three reasons:
#
# 1. They are the security assertions -- that no credential survives a redacted
#    export, and that none reaches the archive on disk. Under Woodpecker's
#    `local` backend the step body is handed to a generated shell wrapper, and a
#    wrapper that swallowed a mid-script `throw` or a non-zero native exit code
#    would report green. A single `.ps1` invocation has one unambiguous exit
#    code, so the check does not depend on wrapper semantics we have not proven.
# 2. The owner can run the exact bytes CI runs, before pushing.
# 3. PowerShell embedded in YAML has already broken this repository once: commit
#    4ff69ee repaired a workflow whose `.\target\release\ct.exe` had been mangled
#    into `.` TAB `arget` CR `elease\ct.exe` in five places, taking the whole
#    file out of the YAML parser. Fewer control characters inside YAML strings
#    is a defect class removed rather than mitigated.

# Version 1.0 deliberately, not Latest. 1.0 catches the misspelled variable,
# which is the mistake that would quietly weaken an assertion here. 2.0 and up
# also prohibit reading a property an object does not have -- and these scripts
# filter heterogeneous JSON (`$_.disposition.kind` over compaction items), where
# a missing property is data, not a defect. Turning that into a failure would
# trade a silent weakening for a noisy false red.
Set-StrictMode -Version 1.0
$ErrorActionPreference = 'Stop'

function Get-RepoRoot {
    # Nested rather than `Join-Path $PSScriptRoot '..' '..'`: the multi-segment
    # form is PowerShell 6+, and the agent's step shell is Windows PowerShell 5.1.
    return (Resolve-Path -LiteralPath (Join-Path (Join-Path $PSScriptRoot '..') '..')).Path
}

function New-CleanDirectory {
    param([Parameter(Mandatory)][string]$Path)

    if (Test-Path -LiteralPath $Path) {
        Remove-Item -LiteralPath $Path -Recurse -Force
    }
    New-Item -ItemType Directory -Force -Path $Path | Out-Null
    return (Resolve-Path -LiteralPath $Path).Path
}

# Every smoke script calls this, and it always isolates *both* agent homes plus
# the archive -- never only the agent whose fixture the script asserts on.
#
# On a fresh GitHub-hosted runner, isolating one home was harmless because the
# other was empty. On a self-hosted agent it is the owner's own machine, where
# `%USERPROFILE%\.claude` holds hundreds of real sessions; those get discovered,
# and `ct sessions` returns its default page of them rather than the fixture.
# The failure is a wrong session id, not an error. Isolating the archive matters
# for the same reason: `CONTEXTTRACE_ARCHIVE` unset resolves to `%LOCALAPPDATA%`,
# and CI must never write into the operator's real archive.
function Use-IsolatedAgentHomes {
    param([Parameter(Mandatory)][string]$Root)

    $codex = New-CleanDirectory (Join-Path $Root 'codex')
    $claude = New-CleanDirectory (Join-Path $Root 'claude')
    $archive = New-CleanDirectory (Join-Path $Root 'archive')

    $env:CODEX_HOME = $codex
    $env:CLAUDE_CONFIG_DIR = $claude
    $env:CONTEXTTRACE_ARCHIVE = $archive

    return [pscustomobject]@{ Codex = $codex; Claude = $claude; Archive = $archive }
}

function New-CiWorkingRoot {
    param([Parameter(Mandatory)][string]$Name)

    # Woodpecker discards the workspace after a pipeline, so anything here is
    # temporary by construction. Named per script so two of them cannot collide.
    $base = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [System.IO.Path]::GetTempPath() }
    return New-CleanDirectory (Join-Path $base "contexttrace-ci-$Name")
}

function Resolve-CtExecutable {
    param([string]$CtExe)

    if ($CtExe) {
        if (-not (Test-Path -LiteralPath $CtExe)) { throw "No ct executable at '$CtExe'." }
        return (Resolve-Path -LiteralPath $CtExe).Path
    }
    if ($env:CT_EXE) {
        if (-not (Test-Path -LiteralPath $env:CT_EXE)) { throw "CT_EXE points at '$($env:CT_EXE)', which does not exist." }
        return (Resolve-Path -LiteralPath $env:CT_EXE).Path
    }

    # Honour CARGO_TARGET_DIR: the Windows pipeline points it at a cache outside
    # the workspace so a Tauri-sized dependency graph is not rebuilt every push.
    $targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path (Get-RepoRoot) 'target' }
    $candidate = Join-Path $targetDir 'release/ct.exe'
    if (-not (Test-Path -LiteralPath $candidate)) {
        throw "No release ct.exe at '$candidate'. Build it with: cargo build --release -p ct-cli"
    }
    return (Resolve-Path -LiteralPath $candidate).Path
}

# Runs `ct` and fails the script unless it exited zero.
#
# The workflow this replaced repeated `if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }`
# after twelve separate invocations. Every one of those was load-bearing and
# invisible if omitted: without it a crashed `ct` yields `$null`, which several
# of the assertions below would read as "nothing found" rather than as a failure.
function Invoke-Ct {
    param(
        [Parameter(Mandatory)][string[]]$Arguments,
        [string]$StandardErrorPath
    )

    if ($StandardErrorPath) {
        # Windows PowerShell 5.1 surfaces a native command's stderr as a
        # NativeCommandError once it is redirected, and $ErrorActionPreference =
        # 'Stop' then makes writing an ordinary progress line fatal. `ct export`
        # reports its redaction summary on stderr by design, and that summary is
        # itself one of the things asserted on. Relax the preference across this
        # call only -- the assignment is function-scoped, so it restores on
        # return. Nothing is weakened: the exit code is still checked below, and
        # that is the real failure signal.
        $ErrorActionPreference = 'Continue'
        $output = & $script:CtExecutable @Arguments 2>$StandardErrorPath
    }
    else {
        $output = & $script:CtExecutable @Arguments
    }
    if ($LASTEXITCODE -ne 0) {
        throw "ct $($Arguments -join ' ') exited with $LASTEXITCODE."
    }
    return $output
}

function Invoke-CtJson {
    param([Parameter(Mandatory)][string[]]$Arguments)

    $output = Invoke-Ct -Arguments $Arguments
    if ($null -eq $output) { throw "ct $($Arguments -join ' ') produced no output to parse as JSON." }
    return ($output | ConvertFrom-Json)
}

# The credential values carried by tests/fixtures/secrets/codex/rollout.jsonl.
# Nothing that reads a session log may ever print or store one of these, so the
# list lives in one place and every script that asserts on it uses this copy.
$script:FixtureSecrets = @(
    'sk-proj-abcdefghijklmnopqrstuvwxyz012345',
    'sk-ant-api03-abcdefghijklmnopqrstuvwxyz012345',
    'ghp_abcdefghijklmnopqrstuvwxyz0123456789AB',
    'AKIAIOSFODNN7EXAMPLE',
    'MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQDemoOnlyFakeKey'
)

function Assert-NoFixtureSecretIn {
    param(
        [Parameter(Mandatory)][AllowEmptyString()][string]$Text,
        [Parameter(Mandatory)][string]$Because,
        [string[]]$Secrets = $script:FixtureSecrets
    )

    foreach ($secret in $Secrets) {
        if ($Text.Contains($secret)) { throw $Because }
    }
}

# Which of the fixture's credentials a given command can emit at all.
#
# "No credential appears in this output" is only evidence if the credential
# could have appeared. It is not automatic that it could: `ct export` does not
# emit `function_call_output` payloads, and the fixture's private key lives only
# in one -- so asserting that key's absence from an export is a check that
# cannot fail, and would keep passing if redaction were removed entirely.
#
# So take the unredacted output as the positive control and assert against the
# set it actually contained. The set is recomputed every run, so it tracks what
# the exporter does rather than what it did when this was written.
function Get-FixtureSecretsPresentIn {
    param([Parameter(Mandatory)][AllowEmptyString()][string]$Text)

    return @($script:FixtureSecrets | Where-Object { $Text.Contains($_) })
}
