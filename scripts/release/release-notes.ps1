function Get-ReleaseNotes {
    param(
        [Parameter(Mandatory = $true)]
        [ValidatePattern('^v\d+\.\d+\.\d+$')]
        [string]$Tag
    )

    $notesPath = Join-Path $PSScriptRoot "../../docs/releases/$Tag.md"
    if (Test-Path -LiteralPath $notesPath -PathType Leaf) {
        # Get-Content carries provider properties that Windows PowerShell 5.1
        # serializes as a JSON object. GitHub's release body must be a string.
        return [System.IO.File]::ReadAllText($notesPath, [Text.Encoding]::UTF8)
    }
    return 'Windows x64 release. Woodpecker validation, packaging and asset integrity checks passed.'
}
