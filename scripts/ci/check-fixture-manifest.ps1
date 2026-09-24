[CmdletBinding()]
param(
    [string]$RepoRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ([string]::IsNullOrWhiteSpace($RepoRoot)) {
    $RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
}

function Get-PropertyValue {
    param(
        [Parameter(Mandatory)]$Object,
        [Parameter(Mandatory)][string]$Name
    )

    $property = $Object.PSObject.Properties[$Name]
    if ($null -eq $property) {
        return $null
    }
    return $property.Value
}

function Get-RelativePath {
    param([Parameter(Mandatory)][string]$Path)

    return $Path.Substring($RepoRoot.Length + 1).Replace('\', '/')
}

$manifestPath = Join-Path $RepoRoot 'tests/fixtures/compatibility.json'
if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
    throw "Fixture compatibility manifest is missing: $manifestPath"
}

$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
if ((Get-PropertyValue $manifest 'schema_version') -ne 1) {
    throw 'Fixture compatibility manifest has an unsupported schema_version.'
}

$lastReviewedText = Get-PropertyValue $manifest 'last_reviewed'
try {
    $lastReviewed = [DateTime]::ParseExact(
        $lastReviewedText,
        'yyyy-MM-dd',
        [Globalization.CultureInfo]::InvariantCulture
    ).Date
}
catch {
    throw "Fixture compatibility manifest has an invalid last_reviewed date: $lastReviewedText"
}
$today = [DateTime]::UtcNow.Date
if ($lastReviewed -gt $today.AddDays(1)) {
    throw "Fixture compatibility manifest is dated in the future: $lastReviewedText"
}
if ($lastReviewed -lt $today.AddDays(-180)) {
    throw "Fixture compatibility manifest is older than 180 days: $lastReviewedText"
}

$surfaceDefinitions = Get-PropertyValue $manifest 'surfaces'
if ($null -eq $surfaceDefinitions) {
    throw 'Fixture compatibility manifest contains no surface definitions.'
}

$entries = @((Get-PropertyValue $manifest 'fixtures'))
if ($entries.Count -eq 0) {
    throw 'Fixture compatibility manifest contains no fixtures.'
}

$manifestPaths = @($entries | ForEach-Object {
        $path = Get-PropertyValue $_ 'path'
        if ([string]::IsNullOrWhiteSpace($path)) {
            throw 'Every fixture manifest entry needs a non-empty path.'
        }
        $path.Replace('\', '/')
    })

$duplicate = @($manifestPaths | Group-Object | Where-Object Count -gt 1)
if ($duplicate.Count -gt 0) {
    throw "Fixture manifest contains duplicate paths: $($duplicate.Name -join ', ')"
}

foreach ($entry in $entries) {
    $relativePath = Get-PropertyValue $entry 'path'
    $relativePath = $relativePath.Replace('/', '\')
    $fixturePath = Join-Path $RepoRoot $relativePath
    if (-not (Test-Path -LiteralPath $fixturePath -PathType Leaf)) {
        throw "Fixture listed in the manifest does not exist: $relativePath"
    }

    $agent = Get-PropertyValue $entry 'agent'
    $surface = Get-PropertyValue $entry 'surface'
    $producerVersion = Get-PropertyValue $entry 'producer_version'
    $provenance = Get-PropertyValue $entry 'provenance'
    $verification = Get-PropertyValue $entry 'verification'
    $capabilityValue = Get-PropertyValue $entry 'capabilities'
    $capabilities = @($capabilityValue)
    if ([string]::IsNullOrWhiteSpace($agent) -or [string]::IsNullOrWhiteSpace($surface)) {
        throw "Fixture entry is missing agent or surface: $relativePath"
    }
    $surfaceDefinition = Get-PropertyValue $surfaceDefinitions $surface
    if ($null -eq $surfaceDefinition) {
        throw "Fixture entry names an undefined surface '$surface': $relativePath"
    }
    if ((Get-PropertyValue $surfaceDefinition 'agent') -ne $agent) {
        throw "Fixture agent '$agent' disagrees with surface '$surface': $relativePath"
    }
    if ($provenance -notin @('synthetic', 'reviewed-local-capture')) {
        throw "Fixture entry has unsupported provenance '$provenance': $relativePath"
    }
    if ([string]::IsNullOrWhiteSpace($verification)) {
        throw "Fixture entry has no verification label: $relativePath"
    }
    if ($null -eq $capabilityValue -or $capabilities.Count -eq 0 -or @($capabilities | Where-Object {
            [string]::IsNullOrWhiteSpace([string]$_)
        }).Count -gt 0) {
        throw "Fixture entry has no capabilities: $relativePath"
    }

    $records = @()
    $lineNumber = 0
    foreach ($line in Get-Content -LiteralPath $fixturePath) {
        $lineNumber++
        if ([string]::IsNullOrWhiteSpace($line)) {
            continue
        }
        try {
            $records += ($line | ConvertFrom-Json)
        }
        catch {
            throw "Fixture is not valid JSONL at ${relativePath}:$lineNumber ($($_.Exception.Message))"
        }
    }
    if ($records.Count -eq 0) {
        throw "Fixture is empty: $relativePath"
    }

    $observedVersion = $null
    foreach ($record in $records) {
        if ($agent -eq 'codex') {
            $payload = Get-PropertyValue $record 'payload'
            if ($null -ne $payload) {
                $observedVersion = Get-PropertyValue $payload 'cli_version'
            }
        }
        elseif ($agent -eq 'claude-code') {
            $observedVersion = Get-PropertyValue $record 'version'
        }
        if (-not [string]::IsNullOrWhiteSpace($observedVersion)) {
            break
        }
    }

    if ([string]::IsNullOrWhiteSpace($observedVersion)) {
        throw "Fixture does not expose a producer version in its records: $relativePath"
    }
    if ($observedVersion -ne $producerVersion) {
        throw "Manifest producer_version '$producerVersion' disagrees with '$observedVersion' in $relativePath"
    }
}

$fixtureRoot = Join-Path $RepoRoot 'tests/fixtures'
$actualPaths = @(Get-ChildItem -LiteralPath $fixtureRoot -Recurse -File -Filter '*.jsonl' | ForEach-Object {
        Get-RelativePath $_.FullName
    } | Sort-Object)
$missingFromManifest = @($actualPaths | Where-Object { $_ -notin $manifestPaths })
$missingFromTree = @($manifestPaths | Where-Object { $_ -notin $actualPaths })
if ($missingFromManifest.Count -gt 0) {
    throw "JSONL fixtures missing from compatibility.json: $($missingFromManifest -join ', ')"
}
if ($missingFromTree.Count -gt 0) {
    throw "Manifest entries missing from the fixture tree: $($missingFromTree -join ', ')"
}

Write-Host "OK: $($entries.Count) JSONL fixtures are present, valid, version-labelled, and catalogued."

