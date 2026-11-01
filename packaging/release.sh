#!/bin/bash
# Publishes a release of the version in package.json at HEAD, from a Mac:
#   1. creates a draft GitHub release v<version> with the notes
#   2. tags HEAD and pushes the tag: GitHub Actions (.github/workflows/release.yml) builds the Arch package
#      and the Linux binary on a native x86_64 runner and attaches them
#   3. meanwhile builds the universal macOS app here (Apple Silicon + Intel) and attaches it
#   4. waits for the Linux build, then publishes the release as the latest
#
# Usage: packaging/release.sh NOTES.md
#
# Needs: gh (logged in), Node/npm, and Rust with both macOS targets:
#   rustup target add aarch64-apple-darwin x86_64-apple-darwin
# Commit the version bump and push it first: the release is built from what is on GitHub.
# To build the Linux downloads locally without releasing, see build-linux-docker.sh.
set -euo pipefail

notes=${1:?usage: packaging/release.sh NOTES.md}
[ -f "$notes" ] || { echo "No notes file at $notes" >&2; exit 1; }
[ "$(uname -s)" = Darwin ] || { echo "release.sh builds the macOS app, so run it on a Mac" >&2; exit 1; }
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

version=$(packaging/check-version.sh)
tag="v$version"
sha=$(git rev-parse HEAD)
git fetch -q origin
git merge-base --is-ancestor "$sha" origin/main || { echo "HEAD isn't on origin/main yet: push it first" >&2; exit 1; }
if git ls-remote --exit-code --tags origin "$tag" >/dev/null; then
  echo "$tag already exists on GitHub" >&2
  exit 1
fi

echo "==> Draft release $tag"
gh release create "$tag" --draft --target "$sha" --title "Kletse RSS $version" --notes-file "$notes"

echo "==> Tag $tag: GitHub Actions builds the Linux downloads"
git tag -a "$tag" "$sha" -m "Kletse RSS $version"
git push -q origin "$tag"

echo "==> macOS (universal), built here meanwhile"
out="$root/packaging/release"
rm -rf "$out"
mkdir -p "$out"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
git archive --format=tar --prefix=kletse-rss-reader/ "$sha" | tar -x -C "$work"
(
  cd "$work/kletse-rss-reader"
  npm ci
  npx tauri build --target universal-apple-darwin
  ditto -c -k --keepParent "src-tauri/target/universal-apple-darwin/release/bundle/macos/Kletse RSS.app" \
    "$out/Kletse-RSS-$version-macos-universal.zip"
)
gh release upload "$tag" "$out/Kletse-RSS-$version-macos-universal.zip" --clobber

echo "==> Waiting for the Linux build"
run=""
until [ -n "$run" ]; do
  run=$(gh run list --workflow release.yml --commit "$sha" --event push --json databaseId -q '.[0].databaseId')
  [ -n "$run" ] || sleep 5
done
# Poll rather than `gh run watch`, which was seen to keep waiting long after a run had finished.
until [ "$(gh run view "$run" --json status -q .status)" = completed ]; do
  sleep 15
done
conclusion=$(gh run view "$run" --json conclusion -q .conclusion)
if [ "$conclusion" != success ]; then
  echo "The Linux build ended with: $conclusion. The release stays a draft: $(gh run view "$run" --json url -q .url)" >&2
  exit 1
fi

echo "==> Publish"
gh release edit "$tag" --draft=false --latest
gh release view "$tag" --json url,assets -q '.url, (.assets[] | "  " + .name)'
