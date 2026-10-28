$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'release-notes.ps1')
$configPath = Join-Path $PSScriptRoot '../../crates/ct-ui/src-tauri/tauri.conf.json'
$version = ([System.IO.File]::ReadAllText($configPath) | ConvertFrom-Json).version
$notes = Get-ReleaseNotes -Tag "v$version"
$request = @{ tag_name = "v$version"; body = $notes } | ConvertTo-Json
$decoded = $request | ConvertFrom-Json
if ($decoded.body -isnot [string]) { throw 'GitHub release body must be a JSON string.' }
if (-not $decoded.body.Contains('## Changes') -or -not $decoded.body.Contains("`n") -or -not $decoded.body.Contains("v$version")) {
    throw 'JSON serialization must preserve the versioned, multiline release notes.'
}
$fallback = @{ body = (Get-ReleaseNotes -Tag 'v999.999.999') } | ConvertTo-Json | ConvertFrom-Json
if ($fallback.body -isnot [string] -or $fallback.body -ne 'Windows x64 release. Woodpecker validation, packaging and asset integrity checks passed.') {
    throw 'An absent notes file must retain the fallback release description.'
}
Write-Output "Release-note API serialization passed on PowerShell $($PSVersionTable.PSVersion)."
