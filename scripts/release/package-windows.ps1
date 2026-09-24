$ErrorActionPreference = 'Stop'

# Woodpecker's Windows local backend can run without a normal user profile.
# Tauri's other build tools and the Woodpecker host still need stable writable
# profile/cache locations even though NSIS itself uses target/.tauri.
$cacheRoot = 'C:/woodpecker-cache/contexttrace'
$env:HOME = Join-Path $cacheRoot 'home'
$env:USERPROFILE = $env:HOME
$env:LOCALAPPDATA = Join-Path $cacheRoot 'localappdata'
$env:APPDATA = Join-Path $cacheRoot 'appdata'
$env:TEMP = Join-Path $cacheRoot 'temp'
$env:TMP = $env:TEMP
$env:RUST_BACKTRACE = '1'

foreach ($directory in @($env:HOME, $env:LOCALAPPDATA, $env:APPDATA, $env:TEMP)) {
    New-Item -ItemType Directory -Force -Path $directory | Out-Null
}

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

& powershell -NoProfile -ExecutionPolicy Bypass -File scripts/ci/check-fixture-manifest.ps1
if ($LASTEXITCODE -ne 0) { throw "Fixture compatibility manifest validation failed with exit code $LASTEXITCODE." }
& powershell -NoProfile -ExecutionPolicy Bypass -File scripts/ci/test-fixture-manifest.ps1
if ($LASTEXITCODE -ne 0) { throw "Fixture compatibility regression validation failed with exit code $LASTEXITCODE." }

$targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { 'target' }
$stageRoot = Join-Path $cacheRoot 'release-assets'
$publishedStage = Join-Path $stageRoot $tag
$stagingRoot = Join-Path $stageRoot '.staging'
$stage = Join-Path $stagingRoot "$tag-$([guid]::NewGuid().ToString('N'))"
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

$uiPackage = @($metadata.packages | Where-Object { $_.name -eq 'ct-ui' })
if ($uiPackage.Count -ne 1) {
    throw "Expected exactly one ct-ui package in cargo metadata; found $($uiPackage.Count)."
}
$uiBinaries = @($uiPackage[0].targets | Where-Object { $_.kind -contains 'bin' })
if ($uiBinaries.Count -ne 1) {
    throw "Expected exactly one ct-ui binary target; found $($uiBinaries.Count)."
}
$desktopExecutable = Join-Path $targetDir "release/$($uiBinaries[0].name).exe"
if (-not (Test-Path -LiteralPath $desktopExecutable -PathType Leaf)) {
    throw "The Tauri desktop executable was not produced at '$desktopExecutable'."
}

$portableStage = Join-Path $stage 'portable'
New-Item -ItemType Directory -Force -Path $portableStage | Out-Null
Copy-Item -LiteralPath $desktopExecutable -Destination (Join-Path $portableStage (Split-Path $desktopExecutable -Leaf))
Copy-Item -LiteralPath 'LICENSE' -Destination (Join-Path $portableStage 'LICENSE')
Set-Content -LiteralPath (Join-Path $portableStage 'PORTABLE.txt') -Encoding ascii -Value @(
    'ContextTrace portable desktop app'
    ''
    "Run $(Split-Path $desktopExecutable -Leaf) without installing ContextTrace."
    'This build still requires the Microsoft Edge WebView2 Runtime on the machine.'
    'The app reads local Codex and Claude Code logs and stores archives in the normal user data location.'
)
Compress-Archive -Path (Join-Path $portableStage '*') -DestinationPath (Join-Path $stage "$assetPrefix-portable.zip")
Remove-Item -LiteralPath $portableStage -Recurse -Force

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

if (Test-Path -LiteralPath $publishedStage) {
    Write-Host "Replacing the previous incomplete or superseded release output: $publishedStage"
    Remove-Item -LiteralPath $publishedStage -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $stageRoot | Out-Null
Move-Item -LiteralPath $stage -Destination $publishedStage

Write-Host "Release assets staged at $publishedStage"
Get-ChildItem -LiteralPath $publishedStage -File | Select-Object Name, Length | Format-Table -AutoSize

