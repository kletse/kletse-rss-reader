#!/bin/bash
# Builds every release download on a Mac, into packaging/release/:
#   kletse-rss-<version>-1-x86_64.pkg.tar.zst   Arch package   (Docker, x86_64 Arch Linux)
#   kletse-rss-<version>-x86_64                 Linux binary   (same build)
#   Kletse-RSS-<version>-macos-universal.zip    macOS app      (Apple Silicon + Intel)
#
# Needs: Docker (Desktop) running, Node/npm, and Rust with both macOS targets:
#   rustup target add aarch64-apple-darwin x86_64-apple-darwin
#
# Usage: packaging/release.sh [git ref]   (default: HEAD)
# Builds what is committed at that ref, not uncommitted changes. Then publish with e.g.
#   gh release create v<version> packaging/release/* --title "Kletse RSS <version>" --notes-file notes.md
set -euo pipefail

ref=${1:-HEAD}
root=$(cd "$(dirname "$0")/.." && pwd)
out="$root/packaging/release"
version=$(git -C "$root" show "$ref:package.json" | sed -n 's/.*"version": "\(.*\)".*/\1/p')
[ -n "$version" ] || { echo "No version in package.json at $ref" >&2; exit 1; }
[ "$(uname -s)" = Darwin ] || { echo "release.sh builds the macOS app too, so run it on a Mac" >&2; exit 1; }

rm -rf "$out"
mkdir -p "$out"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
git -C "$root" archive --format=tar --prefix=kletse-rss-reader/ -o "$work/src.tar" "$ref"
mkdir -p "$work/out"
cp "$root/packaging/linux-in-docker.sh" "$work/"

echo "==> Linux (x86_64, in Docker; slow under emulation on Apple Silicon)"
docker run --rm --platform linux/amd64 -v "$work:/work" archlinux:latest bash /work/linux-in-docker.sh
cp "$work"/out/* "$out/"

echo "==> macOS (universal)"
tar -xf "$work/src.tar" -C "$work"
(
  cd "$work/kletse-rss-reader"
  npm ci
  npx tauri build --target universal-apple-darwin
  ditto -c -k --keepParent "src-tauri/target/universal-apple-darwin/release/bundle/macos/Kletse RSS.app" \
    "$out/Kletse-RSS-$version-macos-universal.zip"
)

echo "==> $out"
ls -l "$out"
