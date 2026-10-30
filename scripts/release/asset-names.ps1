# Invoke the shared asset catalog and emit one name per pipeline item.
function Get-ReleaseAssetNames {
    param(
        [Parameter(Mandatory = $true)][string]$Version,
        [Parameter(Mandatory = $true)][string]$MacMode,
        [Parameter(Mandatory = $true)][string]$LinuxMode
    )

    $json = & node (Join-Path $PSScriptRoot 'release-assets.cjs') names $Version $MacMode $LinuxMode
    if ($LASTEXITCODE -ne 0) { throw 'Unable to determine release assets.' }
    $names = ConvertFrom-Json -InputObject $json
    foreach ($name in $names) {
        if ($name -isnot [string] -or [string]::IsNullOrWhiteSpace($name)) {
            throw 'Release asset names must be nonempty strings.'
        }
        $name
    }
}
