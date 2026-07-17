# Create a version tag and push it so GitHub Actions builds a release.
#
# Usage:
#   pwsh ./scripts/release.ps1 -Version 0.2.0
#   pwsh ./scripts/release.ps1 -Version 0.2.0 -DryRun
#   pwsh ./scripts/release.ps1 -Version 1.0.0-beta.1
#
# What it does:
#   1. Validates the version string
#   2. Updates package.json, tauri.conf.json, and workspace Cargo.toml
#   3. Commits the version bump (unless -SkipCommit)
#   4. Creates annotated tag vX.Y.Z
#   5. Pushes commit + tag → triggers .github/workflows/release.yml
#
# Requirements: git, clean working tree (or -AllowDirty), push access.

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string] $Version,

    [switch] $DryRun,
    [switch] $SkipCommit,
    [switch] $AllowDirty,
    [switch] $ForceTag
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

# Strip optional leading v
$Version = $Version.Trim().TrimStart("v", "V")
if ($Version -notmatch '^\d+\.\d+\.\d+([.-].*)?$') {
    throw "Invalid version '$Version'. Expected semver like 0.2.0 or 1.0.0-beta.1"
}

$tag = "v$Version"
Write-Host "=== 6809Emu release $tag ===" -ForegroundColor Cyan

# Working tree check
$status = git status --porcelain
if ($status -and -not $AllowDirty) {
    Write-Host $status
    throw "Working tree is dirty. Commit/stash changes or pass -AllowDirty."
}

$branch = (git rev-parse --abbrev-ref HEAD).Trim()
if ($branch -ne "master" -and $branch -ne "main") {
    Write-Warning "Current branch is '$branch' (expected master/main)."
}

# Refuse to overwrite an existing tag unless forced
$tagExists = $false
git rev-parse $tag 2>$null | Out-Null
if ($LASTEXITCODE -eq 0) { $tagExists = $true }
if ($tagExists -and -not $ForceTag) {
    throw "Tag $tag already exists. Use -ForceTag to move it (dangerous) or pick a new version."
}

function Set-JsonVersion([string] $path, [string] $ver) {
    $raw = Get-Content -Raw -Path $path
    $obj = $raw | ConvertFrom-Json
    $obj.version = $ver
    # PowerShell's ConvertTo-Json reorders keys; keep a minimal write via node if available
    if (Get-Command node -ErrorAction SilentlyContinue) {
        $env:REL_JSON_PATH = (Resolve-Path $path).Path
        $env:REL_VERSION = $ver
        node -e @"
const fs = require('fs');
const p = process.env.REL_JSON_PATH;
const j = JSON.parse(fs.readFileSync(p, 'utf8'));
j.version = process.env.REL_VERSION;
fs.writeFileSync(p, JSON.stringify(j, null, 2) + '\n');
"@
        Remove-Item Env:REL_JSON_PATH, Env:REL_VERSION -ErrorAction SilentlyContinue
    } else {
        $obj | ConvertTo-Json -Depth 20 | Set-Content -Path $path -Encoding utf8
    }
    Write-Host "  updated $path → $ver"
}

function Set-CargoWorkspaceVersion([string] $path, [string] $ver) {
    $text = Get-Content -Raw -Path $path
    $updated = [regex]::Replace(
        $text,
        '(?m)^(\[workspace\.package\][\s\S]*?^version\s*=\s*)"[^"]+"',
        { param($m) $m.Groups[1].Value + '"' + $ver + '"' }
    )
    if ($updated -eq $text) {
        # Fallback: first bare version = under workspace.package
        $updated = [regex]::Replace($text, '(?m)^(version\s*=\s*)"[^"]+"', {
                param($m) $m.Groups[1].Value + '"' + $ver + '"'
            }, 1)
    }
    if ($updated -ne $text) {
        Set-Content -Path $path -Value $updated -Encoding utf8 -NoNewline
        # Ensure trailing newline
        Add-Content -Path $path -Value "" -Encoding utf8
        Write-Host "  updated $path → $ver"
    } else {
        Write-Warning "Could not patch version in $path"
    }
}

Write-Host "Bumping version files..."
if ($DryRun) {
    Write-Host "  [dry-run] would set version $Version in package.json, tauri.conf.json, Cargo.toml"
} else {
    Set-JsonVersion "package.json" $Version
    Set-JsonVersion "src-tauri/tauri.conf.json" $Version
    Set-CargoWorkspaceVersion "Cargo.toml" $Version
}

if (-not $SkipCommit -and -not $DryRun) {
    git add package.json src-tauri/tauri.conf.json Cargo.toml
    $pending = git diff --cached --name-only
    if ($pending) {
        git commit -m "chore: release $tag"
        Write-Host "Committed version bump."
    } else {
        Write-Host "No version file changes to commit."
    }
}

if ($DryRun) {
    Write-Host "[dry-run] would create tag $tag and push branch + tag"
    Write-Host "Done (dry-run)."
    exit 0
}

if ($tagExists -and $ForceTag) {
    git tag -d $tag
    git push origin ":refs/tags/$tag" 2>$null
}

git tag -a $tag -m "Release $tag"
Write-Host "Created tag $tag"

Write-Host "Pushing $branch and $tag..."
git push origin $branch
git push origin $tag

$remote = (git remote get-url origin).Trim()
$repoWeb = $remote -replace '\.git$', '' -replace 'git@github\.com:', 'https://github.com/' -replace 'https://github.com/', 'https://github.com/'
if ($remote -match 'github\.com[:/](.+?)(?:\.git)?$') {
    $slug = $Matches[1]
    Write-Host ""
    Write-Host "Release workflow started:" -ForegroundColor Green
    Write-Host "  https://github.com/$slug/actions"
    Write-Host "  https://github.com/$slug/releases"
} else {
    Write-Host "Pushed. Check GitHub Actions for the release build."
}
