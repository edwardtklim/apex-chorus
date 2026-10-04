<#
.SYNOPSIS
Creates a verified removable USB from a local folder or ISO. Never downloads an ISO.
.DESCRIPTION
Windows mode supports x64 UEFI only, FAT32, one existing partition up to 32 GiB.
Files >= 4 GiB are refused before formatting (use prepared split installation media).
Payload mode creates an exFAT driver/program USB; it is NOT bootable installation media.
By default only a preview is produced. -Write additionally requires interactive typed approval.
.EXAMPLE
 .\New-InstallUsb.ps1 -Source C:\Media -DriveLetter F -Mode Windows
.EXAMPLE
 .\New-InstallUsb.ps1 -Source C:\Drivers -DriveLetter F -Mode Payload -Write
#>
[CmdletBinding()]
param(
    [string]$Source,
    [ValidatePattern('^[A-Za-z]$')][string]$DriveLetter,
    [ValidateSet('Windows','Payload')][string]$Mode = 'Payload',
    [switch]$Write
)

function Assert-UsbTarget($Disk, $Volume, $Partitions, $Physical) {
    if ($Disk.IsBoot -or $Disk.IsSystem -or $Disk.IsOffline -or $Disk.IsReadOnly) { throw 'System, boot, offline or read-only disk refused.' }
    # USB transport alone does not mean removable: external SSDs are deliberately refused.
    if ([string]$Disk.BusType -ne 'USB' -or [string]$Volume.DriveType -ne 'Removable' -or
        [string]$Physical.MediaType -notmatch 'Removable' -or $Physical.InterfaceType -ne 'USB') {
        throw 'Only positively identified removable USB media is allowed. Fixed disks/SSDs are refused.'
    }
    if (@($Partitions).Count -ne 1 -or $Partitions[0].IsBoot -or $Partitions[0].IsSystem) { throw 'Exactly one non-system partition is required. No repartitioning is performed.' }
    if (-not $Disk.UniqueId -or -not $Volume.UniqueId) { throw 'Disk/volume identity unavailable.' }
}

function Get-UsbTarget([string]$Letter) {
    $part = Get-Partition -DriveLetter $Letter -ErrorAction Stop
    $disk = $part | Get-Disk -ErrorAction Stop
    $volume = $part | Get-Volume -ErrorAction Stop
    $parts = @(Get-Partition -DiskNumber $disk.Number -ErrorAction Stop)
    $physical = Get-CimInstance Win32_DiskDrive -Filter "Index = $($disk.Number)" -ErrorAction Stop
    Assert-UsbTarget $disk $volume $parts $physical
    [pscustomobject]@{ Disk=$disk; Volume=$volume; Partition=$part; Model=$physical.Model
        Identity="$($disk.Number)|$($disk.UniqueId)|$($disk.Size)|$($part.Offset)|$($part.Size)|$($volume.UniqueId)" }
}

function Assert-NoLink([string]$Path) {
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
    while ($null -ne $item) {
        if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Links/junctions are not accepted as a source.' }
        $parent = [IO.Path]::GetDirectoryName($item.FullName)
        if (-not $parent -or $parent -eq $item.FullName) { break }
        $item = Get-Item -LiteralPath $parent -Force -ErrorAction Stop
    }
}

function Get-MediaManifest([string]$Root) {
    # Walk one level at a time so a junction is rejected before it can be traversed.
    foreach ($item in Get-ChildItem -LiteralPath $Root -Force -ErrorAction Stop) {
        if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Source contains a link/junction.' }
        if ($item.PSIsContainer) { Get-MediaManifest $item.FullName }
        else { [pscustomobject]@{ Path=$item.FullName; Bytes=$item.Length; Hash=(Get-FileHash -LiteralPath $item.FullName -Algorithm SHA256).Hash } }
    }
}

function Confirm-UsbErase([string]$Expected, [string]$Answer) {
    if ($Answer -cne $Expected) { throw 'Confirmation did not match. Nothing was erased.' }
}

function Copy-VerifiedMedia([string]$Root, [string]$Destination, $Files) {
    foreach ($file in $Files) {
        $output = Join-Path $Destination $file.Path.Substring($Root.Length)
        New-Item -ItemType Directory -Path (Split-Path $output -Parent) -Force | Out-Null
        Copy-Item -LiteralPath $file.Path -Destination $output -Force
    }
    $verifiedBytes = 0L
    foreach ($file in $Files) {
        $output = Join-Path $Destination $file.Path.Substring($Root.Length)
        $copy = Get-Item -LiteralPath $output -Force
        if ($copy.Length -ne $file.Bytes -or (Get-FileHash -LiteralPath $output -Algorithm SHA256).Hash -ne $file.Hash) {
            throw "Verification failed: $($copy.Name). USB is incomplete; do not use it."
        }
        $verifiedBytes += $copy.Length
    }
    [pscustomobject]@{ Files=@($Files).Count; Bytes=$verifiedBytes; SHA256='All matched' }
}

function Invoke-InstallUsb {
    $ErrorActionPreference = 'Stop'
    if (-not $Source -or -not $DriveLetter) { throw 'Specify -Source and -DriveLetter. Default is preview only; -Write requires typed confirmation.' }
    $letter = $DriveLetter.ToUpperInvariant()
    $target = Get-UsbTarget $letter
    $sourceItem = Get-Item -LiteralPath $Source -Force
    Assert-NoLink $sourceItem.FullName
    if ([IO.Path]::GetPathRoot($sourceItem.FullName) -eq "${letter}:\") { throw 'Source must not be on the target USB.' }
    $mountedHere = $false
    $iso = $null
    try {
        if (-not $sourceItem.PSIsContainer) {
            if ($sourceItem.Extension -ne '.iso') { throw 'Source must be a folder or ISO.' }
            $iso = $sourceItem.FullName
            $diskImage = Get-DiskImage -ImagePath $iso
            if (-not $diskImage.Attached) {
                $diskImage = Mount-DiskImage -ImagePath $iso -Access ReadOnly -PassThru
                $mountedHere = $true
            }
            $isoVolumes = @($diskImage | Get-Volume)
            if ($isoVolumes.Count -ne 1 -or -not $isoVolumes[0].DriveLetter) { throw 'ISO has no single readable volume.' }
            $root = "$($isoVolumes[0].DriveLetter):\"
        } else { $root = $sourceItem.FullName.TrimEnd('\') + '\' }
        $files = @(Get-MediaManifest $root)
        if ($files.Count -eq 0) { throw 'Source is empty.' }
        $bytes = ($files | Measure-Object -Property Bytes -Sum).Sum
        if ($bytes + 64MB -gt $target.Partition.Size) { throw 'Insufficient target capacity including filesystem overhead.' }
        if ($Mode -eq 'Windows') {
            foreach ($required in @('efi\boot\bootx64.efi','sources\boot.wim','setup.exe')) {
                if (-not (Test-Path -LiteralPath (Join-Path $root $required) -PathType Leaf)) { throw "Missing Windows x64 UEFI media file: $required" }
            }
            if (-not ((Test-Path -LiteralPath (Join-Path $root 'sources\install.wim')) -or
                (Test-Path -LiteralPath (Join-Path $root 'sources\install.esd')) -or
                (Test-Path -LiteralPath (Join-Path $root 'sources\install.swm')))) { throw 'Windows installation image is missing.' }
            if ($target.Partition.Size -gt 32GB -or @($files | Where-Object Bytes -ge 4GB).Count) {
                throw 'UEFI FAT32 requires a partition <= 32 GiB and each file < 4 GiB. Prepare split Windows media first.'
            }
        }
        Write-Host "Target: disk $($target.Disk.Number), $($target.Model), $($target.Disk.Size) bytes, ${letter}:"
        Write-Host "Current filesystem: $($target.Volume.FileSystem); used bytes: $($target.Volume.Size - $target.Volume.SizeRemaining)"
        Get-ChildItem -LiteralPath "${letter}:\" -Force | Select-Object Name,Length,Attributes | Format-Table | Out-Host
        Write-Host "ALL current files on ${letter}: will be erased by formatting. Source: $root ($($files.Count) files, $bytes bytes)."
        Write-Host $(if ($Mode -eq 'Windows') { 'Windows x64 UEFI media only. Legacy BIOS unsupported; actual boot must be tested on the destination PC.' } else { 'Driver/program storage only. This USB is NOT bootable Windows installation media.' })
        if (-not $Write) { Write-Host 'Preview only. No target changes. Run with -Write to request interactive approval.'; return }
        $admin = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
        if (-not $admin.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Run PowerShell as administrator to write.' }
        $phrase = "ERASE disk $($target.Disk.Number) ${letter}: $($target.Disk.Size)"
        Confirm-UsbErase $phrase (Read-Host "Type exactly: $phrase")
        $fresh = Get-UsbTarget $letter
        if ($fresh.Identity -cne $target.Identity) { throw 'Target changed since preview. Refusing.' }
        $fs = if ($Mode -eq 'Windows') { 'FAT32' } else { 'exFAT' }
        # Use the freshly verified volume object, never Clear-Disk or a generated shell command.
        $fresh.Volume | Format-Volume -FileSystem $fs -NewFileSystemLabel 'APEX_MEDIA' -Force -Confirm:$false | Out-Null
        $verified = Copy-VerifiedMedia $root "${letter}:\" $files
        Write-Host "VERIFIED: $($verified.Files) copied files, $($verified.Bytes) bytes; SHA256 matched for every file."
        if ($Mode -eq 'Windows') { Write-Host 'Media files verified. Boot success is untested; select UEFI USB in the destination PC boot menu.' }
    } finally {
        if ($mountedHere) { Dismount-DiskImage -ImagePath $iso | Out-Null }
    }
}

if ($MyInvocation.InvocationName -ne '.') { Invoke-InstallUsb }
