$ErrorActionPreference = 'Stop'

if ([string]::IsNullOrWhiteSpace($env:CI_COMMIT_TAG)) {
    throw 'This script must run from a Woodpecker tag pipeline.'
}

$tag = $env:CI_COMMIT_TAG
if ($tag -notmatch '^v\d+\.\d+\.\d+$') {
    throw "Release tag '$tag' must have the form v<major>.<minor>.<patch>."
}

$version = (Get-Content 'crates/ct-ui/src-tauri/tauri.conf.json' -Raw | ConvertFrom-Json).version
if ($tag -ne "v$version") {
    throw "Release tag '$tag' must match the desktop version 'v$version'."
}

$metadata = cargo metadata --no-deps --format-version 1 | ConvertFrom-Json
$workspaceVersions = @($metadata.packages | Select-Object -ExpandProperty version -Unique)
if ($workspaceVersions.Count -ne 1 -or $workspaceVersions[0] -ne $version) {
    throw "All workspace packages must have version '$version'; found: $($workspaceVersions -join ', ')."
}

$targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { 'target' }
$stageRoot = 'C:/woodpecker-cache/contexttrace/release-assets'
$stage = Join-Path $stageRoot $tag
if (Test-Path -LiteralPath $stage) {
    throw "Release staging directory already exists: $stage. Remove it manually before retrying."
}
New-Item -ItemType Directory -Force -Path $stage | Out-Null

Write-Host "Building ContextTrace $version on the Windows Woodpecker agent."
& npm ci --prefix crates/ct-ui
if ($LASTEXITCODE -ne 0) { throw "npm ci failed with exit code $LASTEXITCODE." }

Push-Location crates/ct-ui
try {
    & .\node_modules\.bin\tauri.cmd build --bundles nsis
    if ($LASTEXITCODE -ne 0) { throw "Tauri bundling failed with exit code $LASTEXITCODE." }
}
finally {
    Pop-Location
}

& cargo build --release -p ct-cli
if ($LASTEXITCODE -ne 0) { throw "CLI release build failed with exit code $LASTEXITCODE." }

$installerDirectory = Join-Path $targetDir 'release/bundle/nsis'
$installers = @(Get-ChildItem -LiteralPath $installerDirectory -File -Filter '*.exe')
if ($installers.Count -ne 1) {
    throw "Expected exactly one NSIS installer in '$installerDirectory'; found $($installers.Count)."
}

$assetPrefix = "ContextTrace-$version-windows-x64"
Copy-Item -LiteralPath $installers[0].FullName -Destination (Join-Path $stage "$assetPrefix-setup.exe")

$cliStage = Join-Path $stage 'cli'
New-Item -ItemType Directory -Force -Path $cliStage | Out-Null
Copy-Item -LiteralPath (Join-Path $targetDir 'release/ct.exe') -Destination (Join-Path $cliStage 'ct.exe')
Copy-Item -LiteralPath 'LICENSE' -Destination (Join-Path $cliStage 'LICENSE')
Compress-Archive -Path (Join-Path $cliStage '*') -DestinationPath (Join-Path $stage "$assetPrefix-cli.zip")
Remove-Item -LiteralPath $cliStage -Recurse -Force

$checksums = Get-ChildItem -LiteralPath $stage -File | Sort-Object Name | ForEach-Object {
    "{0} *{1}" -f (Get-FileHash -Algorithm SHA256 $_.FullName).Hash.ToLowerInvariant(), $_.Name
}
Set-Content -LiteralPath (Join-Path $stage 'SHA256SUMS.txt') -Value $checksums -Encoding ascii

Write-Host "Release assets staged at $stage"
Get-ChildItem -LiteralPath $stage -File | Select-Object Name, Length | Format-Table -AutoSize
