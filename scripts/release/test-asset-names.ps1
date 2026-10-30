$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'asset-names.ps1')

$tempRoot = (Resolve-Path -LiteralPath ([System.IO.Path]::GetTempPath())).Path.TrimEnd([char]92)
$testName = "ct-release-names-" + [guid]::NewGuid().ToString('N')
$testRoot = Join-Path $tempRoot $testName
New-Item -ItemType Directory -Path $testRoot | Out-Null
try {
    foreach ($mode in @(
        @{ Mac = '0'; Linux = '0'; Count = 6 },
        @{ Mac = '1'; Linux = '0'; Count = 14 },
        @{ Mac = '0'; Linux = '1'; Count = 10 },
        @{ Mac = '1'; Linux = '1'; Count = 18 }
    )) {
        $assetNames = @(Get-ReleaseAssetNames -Version '1.2.3' -MacMode $mode.Mac -LinuxMode $mode.Linux)
        if ($assetNames.Count -ne $mode.Count) { throw "Unexpected asset count for Mac=$($mode.Mac), Linux=$($mode.Linux): $($assetNames.Count)" }
        $seen = @{}
        foreach ($name in $assetNames) {
            if ($name -isnot [string] -or $seen.ContainsKey($name)) { throw 'Asset names must be unique flat strings.' }
            $seen[$name] = $true
            $path = Join-Path $testRoot $name
            [System.IO.File]::WriteAllText($path, $name)
            $hash = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
            if ($hash -notmatch '^[0-9A-F]{64}$') { throw "Failed to hash fixture $name" }
        }
    }
    Write-Host 'Release asset names are flat and traversable in all four modes.'
}
finally {
    $resolvedRoot = (Resolve-Path -LiteralPath $testRoot).Path
    $resolvedParent = (Resolve-Path -LiteralPath (Split-Path -Parent $resolvedRoot)).Path.TrimEnd([char]92)
    if ($resolvedParent -ne $tempRoot -or (Split-Path -Leaf $resolvedRoot) -ne $testName -or
        $testName -notmatch '^ct-release-names-[0-9a-f]{32}$') {
        throw "Refusing to remove unexpected test directory: $resolvedRoot"
    }
    Remove-Item -LiteralPath $resolvedRoot -Recurse -Force
}
