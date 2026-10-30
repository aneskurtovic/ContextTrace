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
. (Join-Path $PSScriptRoot 'release-notes.ps1')
$releaseNotes = Get-ReleaseNotes -Tag $Tag
$assetPrefix = "ContextTrace-$version-windows-x64"
$installerName = "$assetPrefix-setup.exe"
$macMode = if ($env:CONTEXTTRACE_MACOS_RELEASE -eq '1') { '1' } elseif ([string]::IsNullOrEmpty($env:CONTEXTTRACE_MACOS_RELEASE) -or $env:CONTEXTTRACE_MACOS_RELEASE -eq '0') { '0' } else { throw 'CONTEXTTRACE_MACOS_RELEASE must be 0 or 1.' }
$linuxMode = if ($env:CONTEXTTRACE_LINUX_RELEASE -eq '1') { '1' } elseif ([string]::IsNullOrEmpty($env:CONTEXTTRACE_LINUX_RELEASE) -or $env:CONTEXTTRACE_LINUX_RELEASE -eq '0') { '0' } else { throw 'CONTEXTTRACE_LINUX_RELEASE must be 0 or 1.' }
. (Join-Path $PSScriptRoot 'asset-names.ps1')
$assetNames = @(Get-ReleaseAssetNames -Version $version -MacMode $macMode -LinuxMode $linuxMode)
$stageDirectory = "C:\woodpecker-cache\contexttrace\release-assets\$Tag"
& node scripts/release/release-assets.cjs verify $stageDirectory $Tag $macMode $linuxMode
if ($LASTEXITCODE -ne 0) { throw 'Staged release verification failed.' }
$expectedNames = @($assetNames | Sort-Object)
$expectedHashes = @{}
foreach ($name in $assetNames) {
    $expectedHashes[$name] = (Get-FileHash -LiteralPath (Join-Path $stageDirectory $name) -Algorithm SHA256).Hash.ToLowerInvariant()
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
if ([string]::IsNullOrWhiteSpace($env:CI_COMMIT_SHA) -or $targetCommit -ne $env:CI_COMMIT_SHA) {
    throw "GitHub tag '$Tag' no longer points to the Woodpecker commit."
}
$existingReleases = Invoke-RestMethod -Uri "$apiRoot/releases?per_page=100" -Headers $headers
$existingRelease = @($existingReleases | Where-Object { $_.tag_name -eq $Tag } | Select-Object -First 1)
if ($existingRelease.Count -gt 0) {
    $release = $existingRelease[0]
} else {
    $releaseBody = @{
        tag_name = $Tag
        target_commitish = $targetCommit
        name = "ContextTrace $Tag"
        body = $releaseNotes
        draft = $false
        prerelease = $true
        generate_release_notes = $false
    } | ConvertTo-Json
    if (($releaseBody | ConvertFrom-Json).body -isnot [string]) {
        throw 'Release notes must serialize as a JSON string.'
    }
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
    throw 'GitHub release does not contain the expected release assets.'
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
Write-Host "Stable release published and all assets verified: $($verifiedRelease.html_url)"
