[CmdletBinding()]
param(
    [Parameter()]
    [string]$ArtifactPath
)

$ErrorActionPreference = 'Stop'

function Test-ForbiddenReleasePath {
    param([Parameter(Mandatory)][string]$Path)

    $normalized = $Path.Replace('\', '/')
    $leaf = [IO.Path]::GetFileName($normalized)

    if ($leaf -eq '.env' -or ($leaf -like '.env.*' -and $leaf -ne '.env.example')) {
        return $true
    }

    if ($leaf -match '\.(key|pem|p12|pfx)$') {
        return $true
    }

    if ($leaf -in @(
        'velox_actions.log',
        'velox_checkpoints.txt',
        'velox_daemon.log',
        'velox_timeline.csv',
        'velox_providers.json',
        'velox_models.json',
        'velox_policies.json',
        'velox_ledger.json',
        'velox_ledger.json.lock',
        'velox_pricing.json',
        'velox_profile.json',
        'velox_baseline.json'
    )) {
        return $true
    }

    if ($leaf -like 'velox_ledger.json.corrupt-*') {
        return $true
    }

    # WebView2 user-data folders can contain cookies, login databases, caches,
    # and browsing state. They are runtime profiles, never release assets.
    return $normalized -match '(^|/)[^/]+\.WebView2(/|$)'
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$tracked = @(& git -C $repoRoot ls-files)
if ($LASTEXITCODE -ne 0) {
    throw 'Unable to enumerate tracked files.'
}

$unsafeTracked = @($tracked | Where-Object { Test-ForbiddenReleasePath $_ })
if ($unsafeTracked.Count -gt 0) {
    $sample = @($unsafeTracked | Select-Object -First 20)
    throw "Tracked secret or runtime-state paths are forbidden ($($unsafeTracked.Count) found):`n$($sample -join "`n")"
}

if (-not $ArtifactPath) {
    Write-Output 'Release safety check passed: tracked source paths are clean.'
    exit 0
}

$resolvedArtifact = (Resolve-Path -LiteralPath $ArtifactPath).Path
$artifactEntries = @()

if (Test-Path -LiteralPath $resolvedArtifact -PathType Container) {
    $artifactEntries = @(
        Get-ChildItem -LiteralPath $resolvedArtifact -File -Recurse |
            ForEach-Object { $_.FullName.Substring($resolvedArtifact.Length).TrimStart('\', '/') }
    )
} elseif ([IO.Path]::GetExtension($resolvedArtifact) -eq '.zip') {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [IO.Compression.ZipFile]::OpenRead($resolvedArtifact)
    try {
        $artifactEntries = @($archive.Entries | ForEach-Object { $_.FullName })
    } finally {
        $archive.Dispose()
    }
} else {
    throw 'ArtifactPath must point to a directory or .zip file.'
}

$unsafeArtifactEntries = @(
    $artifactEntries | Where-Object { Test-ForbiddenReleasePath $_ }
)
if ($unsafeArtifactEntries.Count -gt 0) {
    $sample = @($unsafeArtifactEntries | Select-Object -First 20)
    throw "Release artifact contains forbidden paths ($($unsafeArtifactEntries.Count) found):`n$($sample -join "`n")"
}

Write-Output "Release safety check passed: $resolvedArtifact"
