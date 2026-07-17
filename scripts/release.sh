#!/usr/bin/env bash
# Create a version tag and push it so GitHub Actions builds a release.
#
# Usage:
#   ./scripts/release.sh 0.2.0
#   ./scripts/release.sh 0.2.0 --dry-run
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VERSION="${1:-}"
DRY_RUN=false
SKIP_COMMIT=false
ALLOW_DIRTY=false
FORCE_TAG=false

shift || true
while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run) DRY_RUN=true ;;
    --skip-commit) SKIP_COMMIT=true ;;
    --allow-dirty) ALLOW_DIRTY=true ;;
    --force-tag) FORCE_TAG=true ;;
    *) echo "Unknown option: $1"; exit 1 ;;
  esac
  shift
done

if [[ -z "$VERSION" ]]; then
  echo "Usage: $0 <version> [--dry-run] [--skip-commit] [--allow-dirty] [--force-tag]"
  exit 1
fi

VERSION="${VERSION#v}"
if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.-].*)?$ ]]; then
  echo "Invalid version: $VERSION"
  exit 1
fi

TAG="v${VERSION}"
echo "=== 6809Emu release ${TAG} ==="

if [[ -n "$(git status --porcelain)" && "$ALLOW_DIRTY" != true ]]; then
  git status --porcelain
  echo "Working tree is dirty. Commit/stash or pass --allow-dirty."
  exit 1
fi

if git rev-parse "$TAG" >/dev/null 2>&1; then
  if [[ "$FORCE_TAG" != true ]]; then
    echo "Tag $TAG already exists. Use --force-tag or pick a new version."
    exit 1
  fi
fi

set_json_version() {
  local path="$1"
  node -e "
    const fs = require('fs');
    const p = JSON.parse(fs.readFileSync('$path','utf8'));
    p.version = '$VERSION';
    fs.writeFileSync('$path', JSON.stringify(p, null, 2) + '\n');
  "
  echo "  updated $path → $VERSION"
}

if [[ "$DRY_RUN" == true ]]; then
  echo "[dry-run] would bump to $VERSION and push tag $TAG"
  exit 0
fi

echo "Bumping version files..."
set_json_version package.json
set_json_version src-tauri/tauri.conf.json

# workspace.package.version in root Cargo.toml
if grep -q '\[workspace.package\]' Cargo.toml; then
  perl -i -0pe "s/(\[workspace\.package\][^\[]*?^version\s*=\s*)\"[^\"]+\"/\${1}\"$VERSION\"/ms" Cargo.toml
  echo "  updated Cargo.toml → $VERSION"
fi

if [[ "$SKIP_COMMIT" != true ]]; then
  git add package.json src-tauri/tauri.conf.json Cargo.toml
  if ! git diff --cached --quiet; then
    git commit -m "chore: release $TAG"
    echo "Committed version bump."
  fi
fi

if git rev-parse "$TAG" >/dev/null 2>&1 && [[ "$FORCE_TAG" == true ]]; then
  git tag -d "$TAG"
  git push origin ":refs/tags/$TAG" || true
fi

git tag -a "$TAG" -m "Release $TAG"
BRANCH="$(git rev-parse --abbrev-ref HEAD)"
echo "Pushing $BRANCH and $TAG..."
git push origin "$BRANCH"
git push origin "$TAG"

echo ""
echo "Release workflow started — check GitHub Actions / Releases."
