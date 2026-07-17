# Download Microsoft BASIC ROM images used by the CoCo 2 / Dragon 32 profiles.
# Source: Color Computer Archive (XRoar / MAME-MESS layouts).
# Copyright remains with Microsoft / Tandy / Dragon Data.

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$dest = Join-Path $root "crates\m6809-machine\roms"
$staging = Join-Path $root "roms"

New-Item -ItemType Directory -Force -Path $dest | Out-Null
New-Item -ItemType Directory -Force -Path $staging | Out-Null

$files = @(
    @{
        Uri  = "https://colorcomputerarchive.com/repo/ROMs/XRoar/CoCo/BASIC_OS/bas12.rom"
        Out  = Join-Path $dest "bas12.rom"
        Size = 8192
    },
    @{
        Uri  = "https://colorcomputerarchive.com/repo/ROMs/XRoar/CoCo/BASIC_OS/extbas11.rom"
        Out  = Join-Path $dest "extbas11.rom"
        Size = 8192
    }
)

foreach ($f in $files) {
    Write-Host "Downloading $($f.Uri) ..."
    Invoke-WebRequest -Uri $f.Uri -OutFile $f.Out -UseBasicParsing
    $len = (Get-Item $f.Out).Length
    if ($len -ne $f.Size) {
        throw "Unexpected size for $($f.Out): $len (expected $($f.Size))"
    }
}

$zip = Join-Path $staging "dragon32.zip"
Write-Host "Downloading dragon32.zip ..."
Invoke-WebRequest -Uri "https://colorcomputerarchive.com/repo/ROMs/MAME-MESS/dragon32.zip" -OutFile $zip -UseBasicParsing
$extract = Join-Path $staging "dragon32"
if (Test-Path $extract) { Remove-Item -Recurse -Force $extract }
Expand-Archive -Path $zip -DestinationPath $extract -Force
Copy-Item (Join-Path $extract "d32.rom") (Join-Path $dest "d32.rom") -Force

Write-Host "ROMs ready in $dest"
Get-ChildItem $dest | Format-Table Name, Length
