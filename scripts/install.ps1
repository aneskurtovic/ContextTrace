param()

$ErrorActionPreference = 'Stop'

$repository = 'aneskurtovic/ContextTrace'
$apiHeaders = @{
    Accept = 'application/vnd.github+json'
    'User-Agent' = 'ContextTrace-Installer'
}
$releaseUri = "https://api.github.com/repos/$repository/releases/latest"
$workDirectory = Join-Path ([System.IO.Path]::GetTempPath()) ("ContextTrace-install-" + [guid]::NewGuid().ToString('N'))

try {
    $release = Invoke-RestMethod -Uri $releaseUri -Headers $apiHeaders
    $version = [string]$release.tag_name
    if ($version -notmatch '^v(?<version>\d+\.\d+\.\d+)$') {
        throw "The latest stable release has an unexpected tag: '$version'."
    }
    $releaseVersion = $Matches.version

    $assetPrefix = "ContextTrace-$releaseVersion-windows-x64"
    $installerName = "$assetPrefix-setup.exe"
    $checksumsName = 'SHA256SUMS.txt'
    $assetsByName = @{}
    foreach ($asset in $release.assets) {
        $assetsByName[[string]$asset.name] = [string]$asset.browser_download_url
    }

    foreach ($requiredAsset in @($installerName, $checksumsName)) {
        if (-not $assetsByName.ContainsKey($requiredAsset)) {
            throw "Release $version is missing required asset '$requiredAsset'."
        }
    }

    New-Item -ItemType Directory -Path $workDirectory | Out-Null
    $installerPath = Join-Path $workDirectory $installerName
    $checksumsPath = Join-Path $workDirectory $checksumsName
    Invoke-WebRequest -Uri $assetsByName[$installerName] -OutFile $installerPath -UseBasicParsing
    Invoke-WebRequest -Uri $assetsByName[$checksumsName] -OutFile $checksumsPath -UseBasicParsing

    $checksumPattern = '(?m)^([0-9a-fA-F]{64}) \*?' + [regex]::Escape($installerName) + '\s*$'
    $checksumMatch = [regex]::Match((Get-Content -LiteralPath $checksumsPath -Raw), $checksumPattern)
    if (-not $checksumMatch.Success) {
        throw "The release checksum file does not contain an entry for '$installerName'."
    }

    $actualHash = (Get-FileHash -LiteralPath $installerPath -Algorithm SHA256).Hash
    if ($actualHash -ne $checksumMatch.Groups[1].Value) {
        throw 'The downloaded installer does not match its published SHA-256 checksum.'
    }

    Write-Host "Installing ContextTrace $releaseVersion for the current Windows user..."
    $process = Start-Process -FilePath $installerPath -Wait -PassThru
    if ($process.ExitCode -ne 0) {
        throw "The ContextTrace installer exited with code $($process.ExitCode)."
    }
    Write-Host 'ContextTrace installation finished.'
}
finally {
    if (Test-Path -LiteralPath $workDirectory) {
        Remove-Item -LiteralPath $workDirectory -Recurse -Force
    }
}
