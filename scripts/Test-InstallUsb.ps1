# Tests never format a disk; data copying is confined to a unique temp directory.
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'New-InstallUsb.ps1')
function Expect-Rejected([scriptblock]$Action) {
    $rejected=$false
    try { & $Action } catch { $rejected=$true }
    if (-not $rejected) { throw 'Unsafe case was accepted.' }
}
$disk=[pscustomobject]@{IsBoot=$false;IsSystem=$false;IsOffline=$false;IsReadOnly=$false;BusType='USB';UniqueId='test'}
$volume=[pscustomobject]@{DriveType='Removable';UniqueId='test-volume'}
$parts=@([pscustomobject]@{IsBoot=$false;IsSystem=$false})
$physical=[pscustomobject]@{MediaType='Removable Media';InterfaceType='USB'}
Assert-UsbTarget $disk $volume $parts $physical
foreach ($flag in @('IsBoot','IsSystem','IsOffline','IsReadOnly')) {
    $disk.$flag=$true
    Expect-Rejected { Assert-UsbTarget $disk $volume $parts $physical }
    $disk.$flag=$false
}
$volume.DriveType='Fixed'
Expect-Rejected { Assert-UsbTarget $disk $volume $parts $physical }
$volume.DriveType='Removable'; $physical.MediaType='Fixed hard disk media'
Expect-Rejected { Assert-UsbTarget $disk $volume $parts $physical }
$physical.MediaType='Removable Media'; $disk.BusType='SATA'
Expect-Rejected { Assert-UsbTarget $disk $volume $parts $physical }
$disk.BusType='USB'
Expect-Rejected { Assert-UsbTarget $disk $volume @($parts[0],$parts[0]) $physical }
Expect-Rejected { Confirm-UsbErase 'ERASE disk 8 F: 16000000' '' }
Expect-Rejected { Confirm-UsbErase 'ERASE disk 8 F: 16000000' 'yes' }
Expect-Rejected { Confirm-UsbErase 'ERASE disk 8 F: 16000000' 'ERASE disk 9 F: 16000000' }
Confirm-UsbErase 'ERASE disk 8 F: 16000000' 'ERASE disk 8 F: 16000000'
$testRoot=Join-Path ([IO.Path]::GetTempPath()) ('apex-usb-test-'+[guid]::NewGuid().ToString('N'))
$src=Join-Path $testRoot 'source'; $dst=Join-Path $testRoot 'destination'
New-Item -ItemType Directory -Path (Join-Path $src 'nested'),$dst -Force | Out-Null
Set-Content -LiteralPath (Join-Path $src 'nested\driver.txt') -Value 'local fixture'
Set-Content -LiteralPath (Join-Path $src 'program.txt') -Value 'another fixture'
Assert-NoLink $src
$manifest=@(Get-MediaManifest $src)
$result=Copy-VerifiedMedia ($src+'\') $dst $manifest
if ($result.Files -ne 2 -or $result.Bytes -ne ($manifest | Measure-Object Bytes -Sum).Sum) { throw 'Copy verification count/size mismatch.' }
$manifest[0].Hash='BAD-HASH'
Expect-Rejected { Copy-VerifiedMedia ($src+'\') $dst $manifest }
# Junction rejection must occur before traversal, including a link passed as the root.
$junction=Join-Path $src 'linked'
New-Item -ItemType Junction -Path $junction -Target $dst | Out-Null
Expect-Rejected { Get-MediaManifest $src }
Expect-Rejected { Assert-NoLink $junction }
Write-Host "PASS: system/fixed disks, unknown input, multiple partitions, source links rejected; count/size/SHA256 copy checks passed. Fixtures: $testRoot"
