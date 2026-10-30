# Shared helpers for CI smoke scripts. Dot-source, do not run.
# Individual scripts provide an unambiguous exit code for security assertions
# under the Woodpecker shell wrapper, including on Windows PowerShell 5.1.
# Strict mode 1.0 catches undefined variables while allowing missing properties
# in heterogeneous JSON; higher versions reject those properties.
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

# Isolate both agent homes and the archive so self-hosted CI neither reads
# the operator's sessions nor writes to the operator's archive.
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
