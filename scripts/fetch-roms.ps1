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

$hex = Join-Path $staging "ExBasROM.hex"
Write-Host "Downloading Grant Searle ExBasROM.hex ..."
Invoke-WebRequest -Uri "https://raw.githubusercontent.com/jefftranter/6809/master/sbc/exbasrom/ExBasROM.hex" -OutFile $hex -UseBasicParsing
$bin = New-Object byte[] 16384
for ($i = 0; $i -lt 16384; $i++) { $bin[$i] = 0xFF }
Get-Content $hex | ForEach-Object {
    if ($_ -notmatch '^:([0-9A-Fa-f]{2})([0-9A-Fa-f]{4})([0-9A-Fa-f]{2})') { return }
    $len = [Convert]::ToInt32($Matches[1], 16)
    $addr = [Convert]::ToInt32($Matches[2], 16)
    $type = [Convert]::ToInt32($Matches[3], 16)
    if ($type -ne 0) { return }
    $data = $_.Substring(9, $len * 2)
    for ($i = 0; $i -lt $len; $i++) {
        $off = $addr + $i - 0xC000
        if ($off -ge 0 -and $off -lt 16384) {
            $bin[$off] = [Convert]::ToByte($data.Substring($i * 2, 2), 16)
        }
    }
}
$exOut = Join-Path $dest "exbasrom.bin"
[IO.File]::WriteAllBytes($exOut, $bin)
if ((Get-Item $exOut).Length -ne 16384) {
    throw "Unexpected size for exbasrom.bin"
}

# Speech chipset mask ROMs (SP0256-AL2 + CTS256A-AL2).
# IP belongs to General Instrument / Microchip; fetch only where permitted.
$speech = @(
    @{
        Uri  = "http://spatula-city.org/~im14u2c/sp0256-al2/al2.bin"
        Out  = Join-Path $dest "sp0256-al2.bin"
        Size = 2048
    },
    @{
        Uri  = "https://raw.githubusercontent.com/palazzol/TMS7xxx_dumper/main/software/dumps/cts256a.bin"
        Out  = Join-Path $dest "cts256a.bin"
        Size = 4096
    }
)
foreach ($f in $speech) {
    Write-Host "Downloading $($f.Uri) ..."
    Invoke-WebRequest -Uri $f.Uri -OutFile $f.Out -UseBasicParsing
    $len = (Get-Item $f.Out).Length
    if ($len -ne $f.Size) {
        throw "Unexpected size for $($f.Out): $len (expected $($f.Size))"
    }
}

Write-Host "ROMs ready in $dest"
Get-ChildItem $dest | Format-Table Name, Length
