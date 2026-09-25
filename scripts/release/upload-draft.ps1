param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^v\d+\.\d+\.\d+$')]
    [string]$Tag
)

$ErrorActionPreference = 'Stop'

$owner = 'aneskurtovic'
$repository = 'ContextTrace'
$token = $env:GITHUB_RELEASE_TOKEN
if ([string]::IsNullOrWhiteSpace($token)) {
    throw 'GITHUB_RELEASE_TOKEN must be configured as a protected Woodpecker secret.'
}

$version = $Tag.Substring(1)
$assetPrefix = "ContextTrace-$version-windows-x64"
$installerName = "$assetPrefix-setup.exe"
$assetNames = @(
    $installerName,
    "$installerName.sig",
    'latest.json',
    "$assetPrefix-portable.zip",
    "$assetPrefix-cli.zip",
    'SHA256SUMS.txt'
)
$stageDirectory = "C:\woodpecker-cache\contexttrace\release-assets\$Tag"
if (-not (Test-Path -LiteralPath $stageDirectory -PathType Container)) {
    throw "No staged release assets were found at '$stageDirectory'."
}

$files = Get-ChildItem -LiteralPath $stageDirectory -File
$actualNames = @($files | ForEach-Object { $_.Name } | Sort-Object)
$expectedNames = @($assetNames | Sort-Object)
if (Compare-Object -ReferenceObject $expectedNames -DifferenceObject $actualNames) {
    throw "The staged directory must contain exactly these six assets: $($assetNames -join ', ')."
}

$checksums = Get-Content -LiteralPath (Join-Path $stageDirectory 'SHA256SUMS.txt')
$expectedHashes = @{}
foreach ($name in $assetNames | Where-Object { $_ -ne 'SHA256SUMS.txt' }) {
    $entry = @($checksums | Where-Object { $_ -match ('^[0-9a-fA-F]{64} \*?' + [regex]::Escape($name) + '$') })
    if ($entry.Count -ne 1) {
        throw "SHA256SUMS.txt must contain exactly one checksum for '$name'."
    }
    $expectedHash = [regex]::Match($entry[0], '^[0-9a-fA-F]{64}').Value
    $actualHash = (Get-FileHash -LiteralPath (Join-Path $stageDirectory $name) -Algorithm SHA256).Hash
    if ($actualHash -ne $expectedHash) {
        throw "The staged asset '$name' does not match SHA256SUMS.txt."
    }
    $expectedHashes[$name] = $actualHash.ToLowerInvariant()
}
$expectedHashes['SHA256SUMS.txt'] = (Get-FileHash -LiteralPath (Join-Path $stageDirectory 'SHA256SUMS.txt') -Algorithm SHA256).Hash.ToLowerInvariant()

$manifestPath = Join-Path $stageDirectory 'latest.json'
$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
$installerUrl = "https://github.com/$owner/$repository/releases/download/$Tag/$installerName"
if ($manifest.version -ne $version -or $manifest.platforms.'windows-x86_64'.url -ne $installerUrl) {
    throw 'latest.json does not match this tag and its versioned installer URL.'
}
$installerSignature = (Get-Content -LiteralPath (Join-Path $stageDirectory "$installerName.sig") -Raw).Trim()
if ($manifest.platforms.'windows-x86_64'.signature -ne $installerSignature) {
    throw 'latest.json installer signature does not match the staged .sig file.'
}

$headers = @{
    Accept = 'application/vnd.github+json'
    Authorization = "Bearer $token"
    'X-GitHub-Api-Version' = '2022-11-28'
    'User-Agent' = 'ContextTrace-Woodpecker-Release'
}
$apiRoot = "https://api.github.com/repos/$owner/$repository"

function Get-GitHubFailureDetails {
    param([Parameter(Mandatory = $true)]$ErrorRecord)

    $response = $ErrorRecord.Exception.Response
    if ($null -eq $response) {
        return $ErrorRecord.Exception.Message
    }

    $body = ''
    try {
        $stream = $response.GetResponseStream()
        if ($null -ne $stream) {
            $reader = [System.IO.StreamReader]::new($stream)
            try { $body = $reader.ReadToEnd() } finally { $reader.Dispose() }
        }
    } catch {
        $body = "Unable to read response body: $($_.Exception.Message)"
    }

    $status = [int]$response.StatusCode
    $requestId = $response.Headers['X-GitHub-Request-Id']
    $details = "GitHub API returned HTTP $status"
    if (-not [string]::IsNullOrWhiteSpace($requestId)) {
        $details += " (request ID $requestId)"
    }
    if (-not [string]::IsNullOrWhiteSpace($body)) {
        $details += ": $body"
    }
    return $details
}

# Refuse to create a release if the tag is missing or it already has a
# published release. Reuse an existing draft so a transient upload failure can
# be retried without replacing any already-uploaded asset.
$tagRef = Invoke-RestMethod -Uri "$apiRoot/git/ref/tags/$Tag" -Headers $headers
$existingReleases = Invoke-RestMethod -Uri "$apiRoot/releases?per_page=100" -Headers $headers
$existingRelease = @($existingReleases | Where-Object { $_.tag_name -eq $Tag } | Select-Object -First 1)
if ($existingRelease.Count -gt 0) {
    $release = $existingRelease[0]
    if (-not $release.draft) {
        throw "A published release already exists for $Tag. Refusing to modify it."
    }
} else {
    $releaseBody = @{
        tag_name = $Tag
        target_commitish = [string]$tagRef.object.sha
        name = "ContextTrace $Tag"
        body = "Windows x64 release candidate. Validate the downloaded installer, updater feed, portable app and CLI before publishing this draft."
        draft = $true
        prerelease = $false
        generate_release_notes = $false
    } | ConvertTo-Json
    try {
        $release = Invoke-RestMethod -Method Post -Uri "$apiRoot/releases" -Headers $headers -ContentType 'application/json' -Body $releaseBody
    } catch {
        $createFailure = Get-GitHubFailureDetails -ErrorRecord $_

        # A server error can be returned after GitHub has created the draft.
        # Re-read before failing so reruns remain safe and can resume that draft.
        try {
            $releasesAfterFailure = Invoke-RestMethod -Uri "$apiRoot/releases?per_page=100" -Headers $headers
        } catch {
            $lookupFailure = Get-GitHubFailureDetails -ErrorRecord $_
            throw "Failed to create the GitHub draft release. $createFailure. Could not check whether GitHub created it: $lookupFailure"
        }
        $createdDraft = @($releasesAfterFailure | Where-Object { $_.tag_name -eq $Tag } | Select-Object -First 1)
        if ($createdDraft.Count -gt 0 -and $createdDraft[0].draft) {
            $release = $createdDraft[0]
            Write-Warning "GitHub returned an error while creating the draft, but the draft exists and will be resumed. $createFailure"
        } elseif ($createdDraft.Count -gt 0) {
            throw "GitHub returned an error while creating the draft, and a published release now exists for $Tag. It will not be modified. $createFailure"
        } else {
            throw "Failed to create the GitHub draft release. $createFailure"
        }
    }
}

$uploadBase = $release.upload_url -replace '\{\?name,label\}$', ''
$existingAssets = @{}
foreach ($asset in $release.assets) {
    $existingAssets[[string]$asset.name] = $asset
}
$unexpectedAssets = @($existingAssets.Keys | Where-Object { $_ -notin $assetNames })
if ($unexpectedAssets.Count -gt 0) {
    throw "Draft contains unexpected assets: $($unexpectedAssets -join ', '). Remove the draft and retry."
}

foreach ($name in $assetNames) {
    if ($existingAssets.ContainsKey($name)) {
        $expectedDigest = "sha256:$($expectedHashes[$name])"
        if ($existingAssets[$name].digest -ne $expectedDigest) {
            throw "Draft asset '$name' already exists but does not match the staged file. Remove the draft and retry."
        }
        Write-Host "Already uploaded and verified: $name"
        continue
    }

    $path = Join-Path $stageDirectory $name
    $encodedName = [uri]::EscapeDataString($name)
    $contentType = switch ([System.IO.Path]::GetExtension($name).ToLowerInvariant()) {
        '.exe' { 'application/vnd.microsoft.portable-executable' }
        '.sig' { 'text/plain' }
        '.json' { 'application/json' }
        '.zip' { 'application/zip' }
        default { 'text/plain' }
    }
    Invoke-RestMethod -Method Post -Uri "$uploadBase`?name=$encodedName" -Headers $headers -ContentType $contentType -InFile $path | Out-Null
    Write-Host "Uploaded $name"
}

$publishedDraft = Invoke-RestMethod -Uri "$apiRoot/releases/$($release.id)" -Headers $headers
$publishedNames = @($publishedDraft.assets | ForEach-Object { $_.Name } | Sort-Object)
if (Compare-Object -ReferenceObject $expectedNames -DifferenceObject $publishedNames) {
    throw 'GitHub draft does not contain the expected six release assets.'
}
foreach ($asset in $publishedDraft.assets) {
    if ($asset.digest -ne "sha256:$($expectedHashes[[string]$asset.name])") {
        throw "GitHub draft asset '$($asset.name)' does not match the staged package."
    }
}

Write-Host "Draft release ready for acceptance: $($release.html_url)"
Write-Host 'Review and download the draft assets on a separate clean Windows host before publishing.'
