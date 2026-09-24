[CmdletBinding()]
param(
    [string]$RepoRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ([string]::IsNullOrWhiteSpace($RepoRoot)) {
    $RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
}

$validator = Join-Path $PSScriptRoot 'check-fixture-manifest.ps1'
$tempRoot = Join-Path ([IO.Path]::GetTempPath()) ("contexttrace-fixture-manifest-{0}" -f [guid]::NewGuid().ToString('N'))

New-Item -ItemType Directory -Force -Path (Join-Path $tempRoot 'tests') | Out-Null
Copy-Item -LiteralPath (Join-Path $RepoRoot 'tests/fixtures') -Destination (Join-Path $tempRoot 'tests') -Recurse

try {
    $manifestPath = Join-Path $tempRoot 'tests/fixtures/compatibility.json'
    $original = Get-Content -LiteralPath $manifestPath -Raw
    $cases = @(
        @{
            Name = 'missing capabilities'
            Mutate = { param($manifest) $manifest.fixtures[0].capabilities = $null }
        },
        @{
            Name = 'undefined surface'
            Mutate = { param($manifest) $manifest.fixtures[0].surface = 'future-surface' }
        },
        @{
            Name = 'stale review date'
            Mutate = { param($manifest) $manifest.last_reviewed = '2000-01-01' }
        }
    )

    foreach ($case in $cases) {
        $manifest = ConvertFrom-Json -InputObject $original
        & $case.Mutate $manifest
        $manifest | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $manifestPath -Encoding utf8

        $previousPreference = $ErrorActionPreference
        try {
            $ErrorActionPreference = 'Continue'
            & powershell -NoProfile -ExecutionPolicy Bypass -File $validator -RepoRoot $tempRoot 2>&1 | Out-Null
            $validatorExitCode = $LASTEXITCODE
        }
        finally {
            $ErrorActionPreference = $previousPreference
        }
        if ($validatorExitCode -eq 0) {
            throw "Fixture validator accepted the negative case: $($case.Name)"
        }
        Write-Host "OK: validator rejected $($case.Name)."
    }
}
finally {
    if (Test-Path -LiteralPath $tempRoot) {
        Remove-Item -LiteralPath $tempRoot -Recurse -Force
    }
}

