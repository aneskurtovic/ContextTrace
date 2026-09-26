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
    "$assetPrefix-portable.zip",
    "$assetPrefix-cli.zip",
    'SHA256SUMS.txt',
    'latest.json'
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

# Stage a public prerelease only after all Woodpecker validation and package
# steps passed. The updater ignores prereleases; after all six assets are
# uploaded and their digests verified, the final API call promotes it to stable.
$tagRef = Invoke-RestMethod -Uri "$apiRoot/git/ref/tags/$Tag" -Headers $headers
$tagObject = $tagRef.object
while ($tagObject.type -eq 'tag') {
    # Annotated Git tags point to a tag object, which in turn points to the
    # commit. GitHub's release API requires the commit SHA for target_commitish.
    $tagObject = (Invoke-RestMethod -Uri "$apiRoot/git/tags/$($tagObject.sha)" -Headers $headers).object
}
if ($tagObject.type -ne 'commit' -or [string]::IsNullOrWhiteSpace([string]$tagObject.sha)) {
    throw "GitHub tag '$Tag' does not resolve to a commit."
}
$targetCommit = [string]$tagObject.sha
$existingReleases = Invoke-RestMethod -Uri "$apiRoot/releases?per_page=100" -Headers $headers
$existingRelease = @($existingReleases | Where-Object { $_.tag_name -eq $Tag } | Select-Object -First 1)
if ($existingRelease.Count -gt 0) {
    $release = $existingRelease[0]
} else {
    $releaseBody = @{
        tag_name = $Tag
        target_commitish = $targetCommit
        name = "ContextTrace $Tag"
        body = "Windows x64 release. Woodpecker validation, packaging and asset integrity checks passed."
        draft = $false
        prerelease = $true
        generate_release_notes = $false
    } | ConvertTo-Json
    try {
        $release = Invoke-RestMethod -Method Post -Uri "$apiRoot/releases" -Headers $headers -ContentType 'application/json' -Body $releaseBody
    } catch {
        $createFailure = Get-GitHubFailureDetails -ErrorRecord $_

        # A server error can be returned after GitHub has created the prerelease.
        # Re-read before failing so reruns can safely resume it.
        try {
            $releasesAfterFailure = Invoke-RestMethod -Uri "$apiRoot/releases?per_page=100" -Headers $headers
        } catch {
            $lookupFailure = Get-GitHubFailureDetails -ErrorRecord $_
            throw "Failed to create the GitHub prerelease. $createFailure. Could not check whether GitHub created it: $lookupFailure"
        }
        $createdRelease = @($releasesAfterFailure | Where-Object { $_.tag_name -eq $Tag } | Select-Object -First 1)
        if ($createdRelease.Count -gt 0) {
            $release = $createdRelease[0]
            Write-Warning "GitHub returned an error while creating the prerelease, but the release exists and will be resumed. $createFailure"
        } else {
            throw "Failed to create the GitHub prerelease. $createFailure"
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
    throw "Release contains unexpected assets: $($unexpectedAssets -join ', '). Remove the release and retry."
}

foreach ($name in $assetNames) {
    if ($existingAssets.ContainsKey($name)) {
        $expectedDigest = "sha256:$($expectedHashes[$name])"
        if ($existingAssets[$name].digest -ne $expectedDigest) {
            throw "Release asset '$name' already exists but does not match the staged file. Refusing to replace a published asset."
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

$verifiedRelease = Invoke-RestMethod -Uri "$apiRoot/releases/$($release.id)" -Headers $headers
$publishedNames = @($verifiedRelease.assets | ForEach-Object { $_.Name } | Sort-Object)
if (Compare-Object -ReferenceObject $expectedNames -DifferenceObject $publishedNames) {
    throw 'GitHub release does not contain the expected six release assets.'
}
foreach ($asset in $verifiedRelease.assets) {
    if ($asset.digest -ne "sha256:$($expectedHashes[[string]$asset.name])") {
        throw "GitHub release asset '$($asset.name)' does not match the staged package."
    }
}

$publishBody = @{ draft = $false; prerelease = $false; make_latest = 'true' } | ConvertTo-Json
try {
    # Reassert this on retries too: changing a public prerelease to stable does
    # not always update GitHub's /releases/latest pointer by itself.
    Invoke-RestMethod -Method Patch -Uri "$apiRoot/releases/$($release.id)" -Headers $headers -ContentType 'application/json' -Body $publishBody | Out-Null
} catch {
    $publishFailure = Get-GitHubFailureDetails -ErrorRecord $_
    # Publishing may have succeeded despite an HTTP error. Read back the
    # release before failing so retries stay idempotent.
    try {
        $verifiedRelease = Invoke-RestMethod -Uri "$apiRoot/releases/$($release.id)" -Headers $headers
    } catch {
        $lookupFailure = Get-GitHubFailureDetails -ErrorRecord $_
        throw "Failed to promote the release to stable and latest. $publishFailure. Could not verify its state: $lookupFailure"
    }
    if ($verifiedRelease.draft -or $verifiedRelease.prerelease) {
        throw "Failed to promote the release to stable and latest. $publishFailure"
    }
}

$verifiedRelease = Invoke-RestMethod -Uri "$apiRoot/releases/$($release.id)" -Headers $headers
if ($verifiedRelease.draft -or $verifiedRelease.prerelease) {
    throw 'GitHub release still is not published as a stable release.'
}
$latestRelease = $null
for ($attempt = 1; $attempt -le 5; $attempt++) {
    $latestRelease = Invoke-RestMethod -Uri "$apiRoot/releases/latest" -Headers $headers
    if ($latestRelease.tag_name -eq $Tag) {
        break
    }
    if ($attempt -lt 5) {
        Start-Sleep -Seconds 2
    }
}
if ($null -eq $latestRelease -or $latestRelease.tag_name -ne $Tag) {
    $latestTag = if ($null -eq $latestRelease) { 'unavailable' } else { [string]$latestRelease.tag_name }
    throw "GitHub's stable updater feed still points to '$latestTag' instead of '$Tag'."
}
Write-Host "Stable release published and all six assets verified: $($verifiedRelease.html_url)"
